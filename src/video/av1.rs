//! AV1 decoding with rav1d, the Rust port of dav1d (BSD-2-Clause), as
//! Rerun publishes it (`re_rav1d`). Its hand-written assembly is on for
//! arm64 (built with the system's C compiler); on x86-64 it needs nasm, so
//! a game turns it on itself (the guide's "Video" chapter).
//!
//! rav1d's released crate offers only dav1d's C interface, so this file is
//! the one place with `unsafe`: it owns a decoder context, hands it one
//! temporal unit (one frame's data) at a time and borrows the picture it
//! returns until the picture is dropped. Every call follows dav1d's
//! documented contract; the tests in `video.rs` decode real files through it.

use std::mem::MaybeUninit;
use std::ptr::NonNull;

use rav1d::include::dav1d::data::Dav1dData;
use rav1d::include::dav1d::dav1d::{Dav1dContext, Dav1dLogger, Dav1dSettings};
use rav1d::include::dav1d::headers::{
    DAV1D_MC_BT470BG, DAV1D_MC_BT601, DAV1D_MC_BT2020_CL, DAV1D_MC_BT2020_NCL, DAV1D_MC_SMPTE240,
    DAV1D_PIXEL_LAYOUT_I400, DAV1D_PIXEL_LAYOUT_I420, DAV1D_PIXEL_LAYOUT_I422,
};
use rav1d::include::dav1d::picture::Dav1dPicture;
use rav1d::src::lib::{
    dav1d_close, dav1d_data_create, dav1d_data_unref, dav1d_default_settings, dav1d_flush, dav1d_get_picture,
    dav1d_open, dav1d_picture_unref, dav1d_send_data,
};

use super::yuv::{Matrix, Planes, Subsampling};

/// dav1d's "try again": the decoder wants its output read before it takes
/// more input, or has no picture to give yet. dav1d returns errors as
/// negative `errno` values.
const AGAIN: i32 = -libc::EAGAIN;

/// One AV1 decoder.
pub(crate) struct Av1 {
    /// Always `Some` until dropped: dav1d's handle to its context.
    context: Option<Dav1dContext>,
}

// SAFETY: the context is an `Arc` of rav1d's `Rav1dContext`, which is
// `Send` and `Sync` (rav1d shares it with its own worker threads). Every
// method here takes `&mut self`, so it is never used from two threads at
// once.
unsafe impl Send for Av1 {}
// SAFETY: as above: no method touches the context through `&self`.
unsafe impl Sync for Av1 {}

impl Av1 {
    /// A decoder using up to `threads` threads (1: none of its own). One
    /// frame in flight, so each frame comes out as soon as it is sent.
    pub fn new(threads: usize) -> Result<Self, String> {
        let mut settings = MaybeUninit::<Dav1dSettings>::uninit();
        // SAFETY: `dav1d_default_settings` writes a whole `Dav1dSettings`
        // to the pointer, which is valid for writes; after it the value is
        // initialized.
        let mut settings = unsafe {
            dav1d_default_settings(NonNull::new_unchecked(settings.as_mut_ptr()));
            settings.assume_init()
        };
        settings.n_threads = threads.clamp(1, 8) as i32;
        settings.max_frame_delay = 1;
        // dav1d prints what it finds wrong in a stream to stderr; the
        // errors come back as `Result`s here instead.
        // SAFETY: no callback, so nothing is ever called with the cookie.
        settings.logger = unsafe { Dav1dLogger::new(None, None) };
        let mut context: Option<Dav1dContext> = None;
        // SAFETY: both pointers come from live locals, valid for reads and
        // writes for the call. On success `context` holds the new context.
        let result = unsafe { dav1d_open(Some(NonNull::from(&mut context)), Some(NonNull::from(&mut settings))) };
        match context {
            Some(_) if result.0 == 0 => Ok(Self { context }),
            _ => Err(format!("the AV1 decoder could not start (error {})", result.0)),
        }
    }

    /// Decodes one temporal unit (one Matroska block of an AV1 track) and
    /// returns the picture it shows, if it shows one.
    pub fn decode(&mut self, unit: &[u8], timestamp: i64) -> Result<Option<Picture>, String> {
        if unit.is_empty() {
            return Ok(None);
        }
        let mut data = Dav1dData::default();
        // SAFETY: `data` is a live, empty `Dav1dData`. `dav1d_data_create`
        // gives it a buffer of `unit.len()` bytes and returns a pointer to
        // that buffer (null if it could not allocate).
        let buffer = unsafe { dav1d_data_create(Some(NonNull::from(&mut data)), unit.len()) };
        if buffer.is_null() {
            return Err("the AV1 decoder ran out of memory".into());
        }
        // SAFETY: `buffer` points at `unit.len()` writable bytes that
        // `data` owns, which can't overlap `unit` (a fresh allocation).
        unsafe { std::ptr::copy_nonoverlapping(unit.as_ptr(), buffer, unit.len()) };
        data.m.timestamp = timestamp;
        let mut shown = None;
        loop {
            // SAFETY: the context is live (only `drop` closes it) and `data`
            // is a `Dav1dData` that `dav1d_data_create` filled. dav1d takes
            // what it can and leaves the rest in `data`.
            let sent = unsafe { dav1d_send_data(self.context, Some(NonNull::from(&mut data))) };
            if sent.0 != 0 && sent.0 != AGAIN {
                // SAFETY: `data` is ours to release, consumed or not.
                unsafe { dav1d_data_unref(Some(NonNull::from(&mut data))) };
                return Err(format!("the AV1 data could not be decoded (error {})", sent.0));
            }
            match self.picture() {
                Ok(Some(picture)) => shown = Some(picture),
                Ok(None) => {}
                Err(e) => {
                    // SAFETY: as above.
                    unsafe { dav1d_data_unref(Some(NonNull::from(&mut data))) };
                    return Err(e);
                }
            }
            if data.sz == 0 {
                break;
            }
        }
        Ok(shown)
    }

    /// The next decoded picture, if one is ready.
    fn picture(&mut self) -> Result<Option<Picture>, String> {
        let mut picture = Dav1dPicture::default();
        // SAFETY: the context is live and `picture` is an empty picture
        // dav1d may fill; on success it holds a reference we release in
        // `Picture`'s `drop`.
        let got = unsafe { dav1d_get_picture(self.context, Some(NonNull::from(&mut picture))) };
        match got.0 {
            0 => Ok(Some(Picture(picture))),
            AGAIN => Ok(None),
            e => Err(format!("the AV1 data could not be decoded (error {e})")),
        }
    }

    /// Forgets every frame decoded so far: the next unit sent must start
    /// at a keyframe (after a seek).
    pub fn flush(&mut self) {
        if let Some(context) = self.context {
            // SAFETY: the context is live; flushing has no other contract.
            unsafe { dav1d_flush(context) };
        }
    }
}

impl Drop for Av1 {
    fn drop(&mut self) {
        // SAFETY: `context` came from `dav1d_open` and is closed only here,
        // once; `dav1d_close` sets it to `None`.
        unsafe { dav1d_close(Some(NonNull::from(&mut self.context))) };
    }
}

/// A decoded picture, borrowed from the decoder until dropped.
pub(crate) struct Picture(Dav1dPicture);

impl Picture {
    /// Its planes, or `None` for a format the converter doesn't read
    /// (more than 8 bits per sample).
    pub fn planes(&self) -> Option<Planes<'_>> {
        let p = &self.0;
        let (width, height) = (usize::try_from(p.p.w).ok()?, usize::try_from(p.p.h).ok()?);
        if p.p.bpc != 8 || width == 0 || height == 0 {
            return None;
        }
        let subsampling = match p.p.layout {
            DAV1D_PIXEL_LAYOUT_I400 => Subsampling::Gray,
            DAV1D_PIXEL_LAYOUT_I420 => Subsampling::Both,
            DAV1D_PIXEL_LAYOUT_I422 => Subsampling::Horizontal,
            _ => Subsampling::None,
        };
        let (luma_stride, chroma_stride) = (usize::try_from(p.stride[0]).ok()?, usize::try_from(p.stride[1]).ok()?);
        let chroma_height = if subsampling == Subsampling::Both {
            height.div_ceil(2)
        } else {
            height
        };
        let plane = |i: usize, stride: usize, rows: usize| -> Option<&[u8]> {
            let ptr = p.data[i]?;
            // SAFETY: dav1d's default allocator gives each plane `stride`
            // bytes for every row of its (aligned) height, so `rows` full
            // strides are inside it. The planes stay valid while the
            // picture reference we hold lives, which outlives `&self`.
            Some(unsafe { std::slice::from_raw_parts(ptr.as_ptr().cast::<u8>(), stride * rows) })
        };
        let y = plane(0, luma_stride, height)?;
        let (u, v) = if subsampling == Subsampling::Gray {
            (&[][..], &[][..])
        } else {
            (
                plane(1, chroma_stride, chroma_height)?,
                plane(2, chroma_stride, chroma_height)?,
            )
        };
        // SAFETY: a picture dav1d returned carries its sequence header,
        // valid while the picture lives.
        let header = p.seq_hdr.map(|h| unsafe { h.as_ref() });
        let matrix = match header.map(|h| h.mtrx) {
            Some(DAV1D_MC_BT601 | DAV1D_MC_BT470BG | DAV1D_MC_SMPTE240) => Matrix::Bt601,
            Some(DAV1D_MC_BT2020_NCL | DAV1D_MC_BT2020_CL) => Matrix::Bt2020,
            _ => Matrix::Bt709,
        };
        Some(Planes {
            width,
            height,
            subsampling,
            y,
            u,
            v,
            luma_stride,
            chroma_stride,
            matrix,
            full_range: header.is_some_and(|h| h.color_range != 0),
        })
    }

    /// The timestamp the picture's data was sent with.
    #[cfg(test)]
    pub fn timestamp(&self) -> i64 {
        self.0.m.timestamp
    }
}

impl Drop for Picture {
    fn drop(&mut self) {
        // SAFETY: the picture came from `dav1d_get_picture` and is
        // released only here, once.
        unsafe { dav1d_picture_unref(Some(NonNull::from(&mut self.0))) };
    }
}

//! `Game::pack_limits` sets the caps on what packs unpack to for the packs the
//! engine opens by itself (mod packs, `PackSource::open`/`from_bytes`). One
//! test in its own process: the setting is process-wide.

use ponko::assets::{AssetSource, PackLimits, PackSource};
use ponko::mods::ModPack;
use ponko::prelude::*;

/// A pack of `count` stored files of `len` bytes each, written as `ponko pack` does.
fn pack(count: usize, len: usize) -> Vec<u8> {
    let mut index = Vec::new();
    let mut data = Vec::new();
    for i in 0..count {
        let name = format!("f{i:04}.bin");
        index.extend((name.len() as u16).to_le_bytes());
        index.extend(name.as_bytes());
        index.push(0);
        index.extend((data.len() as u64).to_le_bytes());
        index.extend((len as u64).to_le_bytes());
        index.extend((len as u64).to_le_bytes());
        index.extend(crc32(&vec![1u8; len]).to_le_bytes());
        data.extend(vec![1u8; len]);
    }
    let mut out = b"PONKOPAK".to_vec();
    out.extend(1u32.to_le_bytes());
    out.extend((count as u32).to_le_bytes());
    out.extend((index.len() as u64).to_le_bytes());
    out.extend((data.len() as u64).to_le_bytes());
    out.extend(index);
    out.extend(data);
    out
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = flate2::Crc::new();
    crc.update(bytes);
    crc.sum()
}

#[test]
fn the_game_wide_pack_limits_apply_to_packs_opened_afterwards_up_or_down() {
    assert_eq!(PackLimits::current(), PackLimits::default());
    let bytes = pack(10, 1000);
    assert_eq!(PackSource::from_bytes("a", bytes.clone()).unwrap().len(), 10);

    // Down: ten files are too many, then 1000 bytes are too large for one file.
    let _ = Game::new("limits", 1, 1).pack_limits(PackLimits::default().with_max_entries(9));
    assert_eq!(PackLimits::current().max_entries, 9);
    let err = PackSource::from_bytes("a", bytes.clone()).err().unwrap();
    assert!(err.message().contains("max_entries"), "{}", err.message());
    // A mod pack goes through the same limits, with no change to how it is mounted.
    assert!(ModPack::from_pack_bytes("m", bytes.clone()).is_err());

    let _ = Game::new("limits", 1, 1).pack_limits(PackLimits::default().with_max_entry_size(999));
    let pack = PackSource::from_bytes("a", bytes.clone()).unwrap();
    assert!(
        pack.read("f0000.bin")
            .unwrap_err()
            .to_string()
            .contains("max_entry_size")
    );

    // Up again: the same pack reads, and one pack can still choose its own.
    let _ = Game::new("limits", 1, 1).pack_limits(PackLimits::default().with_max_entry_size(1 << 40));
    assert_eq!(
        PackSource::from_bytes("a", bytes.clone())
            .unwrap()
            .read("f0000.bin")
            .unwrap()
            .len(),
        1000
    );
    let strict = PackLimits::default().with_max_pack_size(5000);
    assert!(PackSource::from_bytes_with_limits("a", bytes, strict).is_err());
}

//! `shared` lends the instance array to the tasks of the parallel compare as
//! atomic words (an `unsafe` cast). This test does what those tasks do, on a
//! few instances so that it runs under Miri (book/src/contributing.md, "Unsafe code"):
//! the full extraction tests take minutes per frame there.

use super::*;

#[test]
fn instances_written_as_shared_words_from_several_threads_are_all_kept() {
    let n = if cfg!(miri) { 8 } else { 256 };
    let at = |i: usize| {
        let mut instance = Instance3D::default();
        instance.model[3][0] = i as f32;
        instance
    };
    let mut instances: Vec<Instance3D> = (0..n).map(at).collect();
    // The `x` of an instance's position, as a word.
    let x = std::mem::offset_of!(Instance3D, model) / size_of::<u32>() + 12;
    let words = shared(&mut instances);
    assert_eq!(words.len(), n * WORDS, "one word per four bytes");
    std::thread::scope(|s| {
        for task in 0..4 {
            s.spawn(move || {
                for i in (task..n).step_by(4) {
                    let slot = &words[i * WORDS..(i + 1) * WORDS];
                    let seen = f32::from_bits(slot[x].load(Ordering::Relaxed));
                    assert_eq!(seen, i as f32, "a task reads what was there");
                    for (w, word) in slot.iter().enumerate() {
                        word.store((i * WORDS + w) as u32, Ordering::Relaxed);
                    }
                }
            });
        }
    });
    let written: &[u32] = bytemuck::cast_slice(&instances);
    assert!(
        written.iter().enumerate().all(|(k, &word)| word == k as u32),
        "every task's writes land in its own instances, and all are kept"
    );
}

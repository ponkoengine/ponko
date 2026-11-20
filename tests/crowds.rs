//! The crowd example headless: its 10,000 people are made as one crowd,
//! and every two seconds an update hands the crowd the next section
//! cheering (the game side of crowds: no window or GPU).
#![cfg(feature = "3d")]

#[path = "../examples/crowd.rs"]
#[allow(dead_code)]
mod example;

#[test]
fn the_crowd_example_makes_its_crowd_and_cheers_section_by_section() {
    let world = example::game().run_headless(5 * 60);
    let plaza = world.resource::<example::Plaza>();
    assert_eq!(plaza.people.len(), example::PEOPLE);
    // Five seconds: sections changed at 2 s and 4 s.
    assert_eq!(plaza.updates, 2, "two updates took");
    assert_eq!(plaza.section, 2);
    let cheering = plaza.people.iter().filter(|p| p.clip == 1).count();
    assert_eq!(cheering, example::PEOPLE / 5, "one section of five cheers");
}

use ponko_ffi::*;
use std::ptr::{null, null_mut};

#[test]
fn components_round_trip_and_stale_entities_stay_dead() {
    unsafe {
        let w = ponko_world_new();
        let mut id = 0;
        assert_eq!(
            ponko_component_register(w, c"position".as_ptr(), 4, 4, &mut id),
            PonkoStatus::PONKO_OK
        );
        let value = 42u32;
        let v = PonkoValue {
            component: id,
            data: (&value as *const u32).cast(),
            size: 4,
        };
        let mut e = 0;
        assert_eq!(ponko_spawn(w, &v, 1, &mut e), PonkoStatus::PONKO_OK);
        let mut out = 0u32;
        assert_eq!(
            ponko_get(w, e, id, (&mut out as *mut u32).cast(), 4),
            PonkoStatus::PONKO_OK
        );
        assert_eq!(out, value);
        assert_eq!(ponko_despawn(w, e), PonkoStatus::PONKO_OK);
        let mut alive = true;
        assert_eq!(ponko_is_alive(w, e, &mut alive), PonkoStatus::PONKO_OK);
        assert!(!alive);
        assert_eq!(ponko_world_free(w), PonkoStatus::PONKO_OK);
    }
}

#[test]
fn clearing_a_world_despawns_every_entity_and_keeps_its_resources() {
    unsafe {
        let w = ponko_world_new();
        let mut id = 0;
        assert_eq!(
            ponko_component_register(w, c"position".as_ptr(), 4, 4, &mut id),
            PonkoStatus::PONKO_OK
        );
        let value = 7u32;
        let v = PonkoValue {
            component: id,
            data: (&value as *const u32).cast(),
            size: 4,
        };
        let mut spawned = [0u64; 3];
        for e in &mut spawned {
            assert_eq!(ponko_spawn(w, &v, 1, e), PonkoStatus::PONKO_OK);
        }
        let score = 12u32;
        assert_eq!(
            ponko_resource_set(w, c"Score".as_ptr(), (&score as *const u32).cast(), 4, 4),
            PonkoStatus::PONKO_OK
        );
        assert_eq!(ponko_world_clear(w), PonkoStatus::PONKO_OK);
        let mut len = 9;
        assert_eq!(ponko_world_len(w, &mut len), PonkoStatus::PONKO_OK);
        assert_eq!(len, 0);
        for e in spawned {
            let mut alive = true;
            assert_eq!(ponko_is_alive(w, e, &mut alive), PonkoStatus::PONKO_OK);
            assert!(!alive);
        }
        let mut kept = 0u32;
        assert_eq!(
            ponko_resource_get(w, c"Score".as_ptr(), (&mut kept as *mut u32).cast(), 4),
            PonkoStatus::PONKO_OK
        );
        assert_eq!(kept, 12, "resources stay");
        // A new entity after the clear gets a fresh id.
        let mut again = 0;
        assert_eq!(ponko_spawn(w, &v, 1, &mut again), PonkoStatus::PONKO_OK);
        assert!(!spawned.contains(&again));
        assert_eq!(ponko_world_clear(null_mut()), PonkoStatus::PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_world_free(w), PonkoStatus::PONKO_OK);
    }
}

#[test]
fn invalid_arguments_are_errors() {
    unsafe {
        let w = ponko_world_new();
        let mut id = 0;
        assert_eq!(
            ponko_component_register(w, c"bad".as_ptr(), 3, 2, &mut id),
            PonkoStatus::PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_component_register(w, c"bad".as_ptr(), 4, 3, &mut id),
            PonkoStatus::PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_world_len(null_mut(), &mut 0), PonkoStatus::PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_spawn(w, null(), 1, &mut 0), PonkoStatus::PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_world_free(w), PonkoStatus::PONKO_OK);
    }
}

#[test]
fn copy_sizes_ids_and_duplicate_values_are_validated() {
    unsafe {
        let w = ponko_world_new();
        let mut id = 99;
        assert_eq!(
            ponko_component_register(w, c"x".as_ptr(), 4, 4, &mut id),
            PonkoStatus::PONKO_OK
        );
        let mut same = 99;
        assert_eq!(
            ponko_component_register(w, c"x".as_ptr(), 4, 4, &mut same),
            PonkoStatus::PONKO_OK
        );
        assert_eq!(id, same);
        assert_eq!(
            ponko_component_register(w, c"x".as_ptr(), 8, 8, &mut same),
            PonkoStatus::PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_component_register(w, c"".as_ptr(), 1, 1, &mut same),
            PonkoStatus::PONKO_INVALID_ARGUMENT
        );
        let mut e = 0;
        assert_eq!(ponko_spawn(w, null(), 0, &mut e), PonkoStatus::PONKO_OK);
        let x = 77u32;
        assert_eq!(
            ponko_set(w, e, id, (&x as *const u32).cast(), 3),
            PonkoStatus::PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_set(w, e, 999, (&x as *const u32).cast(), 4),
            PonkoStatus::PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_set(w, e, id, null(), 4), PonkoStatus::PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_set(w, e, id, (&x as *const u32).cast(), 4), PonkoStatus::PONKO_OK);
        let y = 88u32;
        assert_eq!(ponko_set(w, e, id, (&y as *const u32).cast(), 4), PonkoStatus::PONKO_OK);
        let mut out = 0u32;
        assert_eq!(
            ponko_get(w, e, id, (&mut out as *mut u32).cast(), 3),
            PonkoStatus::PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_get(w, e, id, (&mut out as *mut u32).cast(), 4),
            PonkoStatus::PONKO_OK
        );
        assert_eq!(out, 88);
        let v = PonkoValue {
            component: id,
            data: (&x as *const u32).cast(),
            size: 4,
        };
        assert_eq!(
            ponko_spawn(w, [v, v].as_ptr(), 2, &mut 0),
            PonkoStatus::PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_remove(w, e, id), PonkoStatus::PONKO_OK);
        assert_eq!(ponko_remove(w, e, id), PonkoStatus::PONKO_NOT_FOUND);
        assert_eq!(
            ponko_get(w, e, id, (&mut out as *mut u32).cast(), 4),
            PonkoStatus::PONKO_NOT_FOUND
        );
        assert_eq!(ponko_despawn(w, e), PonkoStatus::PONKO_OK);
        assert_eq!(
            ponko_set(w, e, id, (&x as *const u32).cast(), 4),
            PonkoStatus::PONKO_NOT_FOUND
        );
        assert_eq!(ponko_world_free(w), PonkoStatus::PONKO_OK);
    }
}

struct Visit {
    world: *mut PonkoWorld,
    rows: usize,
    entities: Vec<u64>,
    reentry: Vec<PonkoStatus>,
}
unsafe extern "C" fn update(chunk: *const PonkoChunk, user: *mut std::ffi::c_void) {
    unsafe {
        let c = &*chunk;
        let visit = &mut *user.cast::<Visit>();
        visit.rows += c.count;
        for i in 0..c.count {
            visit.entities.push(ponko_entity_bits(*c.entities.add(i)));
            if c.column_count == 2 {
                let velocity = *(*c.columns).cast::<u32>().add(i);
                *(*c.columns.add(1)).cast::<u32>().add(i) += velocity;
            }
        }
        visit.reentry.push(ponko_world_len(visit.world, &mut 0));
        visit.reentry.push(ponko_world_free(visit.world));
        visit.reentry.push(ponko_despawn(visit.world, visit.entities[0]));
        visit.reentry.push(ponko_world_clear(visit.world));
        visit
            .reentry
            .push(ponko_query(visit.world, null(), 0, Some(update), user));
    }
}

#[test]
fn query_preserves_column_order_mutates_values_and_rejects_reentry() {
    unsafe {
        let w = ponko_world_new();
        let (mut p, mut v) = (0, 0);
        ponko_component_register(w, c"p".as_ptr(), 4, 4, &mut p);
        ponko_component_register(w, c"v".as_ptr(), 4, 4, &mut v);
        let (position, velocity) = (10u32, 3u32);
        let values = [
            PonkoValue {
                component: p,
                data: (&position as *const u32).cast(),
                size: 4,
            },
            PonkoValue {
                component: v,
                data: (&velocity as *const u32).cast(),
                size: 4,
            },
        ];
        let mut entity = 0;
        ponko_spawn(w, values.as_ptr(), 2, &mut entity);
        let mut visit = Visit {
            world: w,
            rows: 0,
            entities: vec![],
            reentry: vec![],
        };
        assert_eq!(
            ponko_query(w, [v, p].as_ptr(), 2, Some(update), (&mut visit as *mut Visit).cast()),
            PonkoStatus::PONKO_OK
        );
        assert_eq!(visit.rows, 1);
        assert_eq!(visit.entities, [entity]);
        assert!(visit.reentry.iter().all(|s| *s == PonkoStatus::PONKO_BUSY));
        let mut out = 0u32;
        assert_eq!(
            ponko_get(w, entity, p, (&mut out as *mut u32).cast(), 4),
            PonkoStatus::PONKO_OK
        );
        assert_eq!(out, 13);
        assert_eq!(
            ponko_query(w, [p, p].as_ptr(), 2, Some(update), null_mut()),
            PonkoStatus::PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_query(w, [999].as_ptr(), 1, Some(update), null_mut()),
            PonkoStatus::PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_query(w, null(), 33, Some(update), null_mut()),
            PonkoStatus::PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_query(w, null(), 0, None, null_mut()),
            PonkoStatus::PONKO_INVALID_ARGUMENT
        );
        visit.rows = 0;
        assert_eq!(
            ponko_query(w, null(), 0, Some(update), (&mut visit as *mut Visit).cast()),
            PonkoStatus::PONKO_OK
        );
        assert_eq!(visit.rows, 1);
        ponko_world_free(w);
    }
}

#[test]
fn empty_world_queries_do_not_call_back() {
    unsafe extern "C" fn count(_: *const PonkoChunk, user: *mut std::ffi::c_void) {
        unsafe {
            *user.cast::<usize>() += 1;
        }
    }
    unsafe {
        let w = ponko_world_new();
        let mut calls = 0usize;
        assert_eq!(
            ponko_query(w, null(), 0, Some(count), (&mut calls as *mut usize).cast()),
            PonkoStatus::PONKO_OK
        );
        assert_eq!(calls, 0);
        ponko_world_free(w);
    }
}

#[test]
fn null_outputs_overflow_counts_and_zero_sized_values_are_handled() {
    unsafe {
        let w = ponko_world_new();
        assert_eq!(ponko_world_free(null_mut()), PonkoStatus::PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_world_len(w, null_mut()), PonkoStatus::PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_is_alive(w, 0, null_mut()), PonkoStatus::PONKO_INVALID_ARGUMENT);
        assert_eq!(
            ponko_component_register(w, null(), 1, 1, &mut 0),
            PonkoStatus::PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_component_register(w, c"x".as_ptr(), 1, 1, null_mut()),
            PonkoStatus::PONKO_INVALID_ARGUMENT
        );
        let invalid_utf8 = [0xffu8, 0];
        assert_eq!(
            ponko_component_register(w, invalid_utf8.as_ptr().cast(), 1, 1, &mut 0),
            PonkoStatus::PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_component_register(w, c"x".as_ptr(), usize::MAX, 1, &mut 0),
            PonkoStatus::PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_spawn(w, null(), usize::MAX, &mut 0),
            PonkoStatus::PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_spawn(w, null(), 0, null_mut()),
            PonkoStatus::PONKO_INVALID_ARGUMENT
        );
        let mut id = 0;
        assert_eq!(
            ponko_component_register(w, c"tag".as_ptr(), 0, 8, &mut id),
            PonkoStatus::PONKO_OK
        );
        let mut dummy = 0u64;
        let value = PonkoValue {
            component: id,
            data: (&dummy as *const u64).cast(),
            size: 0,
        };
        let mut entity = 0;
        assert_eq!(ponko_spawn(w, &value, 1, &mut entity), PonkoStatus::PONKO_OK);
        assert_eq!(
            ponko_get(w, entity, id, (&mut dummy as *mut u64).cast(), 0),
            PonkoStatus::PONKO_OK
        );
        ponko_world_free(w);
    }
}

#[test]
fn last_error_is_thread_local_and_success_preserves_it() {
    unsafe {
        ponko_world_free(null_mut());
    }
    let text = unsafe { std::ffi::CStr::from_ptr(ponko_last_error()) }
        .to_bytes()
        .to_vec();
    assert!(!text.is_empty());
    assert_eq!(ponko_abi_version(), 2);
    let child = std::thread::spawn(|| {
        unsafe { std::ffi::CStr::from_ptr(ponko_last_error()) }
            .to_bytes()
            .to_vec()
    })
    .join()
    .unwrap();
    assert!(child.is_empty());
    assert_eq!(unsafe { std::ffi::CStr::from_ptr(ponko_last_error()) }.to_bytes(), text);
}

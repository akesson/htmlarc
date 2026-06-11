use rkyv::rancor::Error;

use super::{ARENA_CAP, RunIndex, RunRebuilder, RunVec, TERMINATOR};

/// Build a run from `values` the way the parse path does.
fn parsed_run(runs: &mut RunVec, values: &[u16]) -> RunIndex {
    let mut iter = values.iter();
    let start = runs.try_new_run(*iter.next().unwrap()).unwrap();
    for &v in iter {
        assert!(runs.try_append_last(start, v));
    }
    start
}

fn collect(runs: &RunVec, start: RunIndex) -> Vec<u16> {
    runs.run_at(start).collect()
}

#[test]
fn runs_preserve_order_and_dedup_within_a_run() {
    let mut runs = RunVec::default();
    let r0 = parsed_run(&mut runs, &[5, 1, 3, 1, 5]);
    let r1 = parsed_run(&mut runs, &[1, 1]);

    // dedup is per run: r0 keeps first-seen order, r1 still holds its own 1.
    assert_eq!(collect(&runs, r0), vec![5, 1, 3]);
    assert_eq!(collect(&runs, r1), vec![1]);
    // arena layout: r0's three values + terminator, then r1.
    assert_eq!(r0.as_u16(), 0);
    assert_eq!(r1.as_u16(), 4);
    assert_eq!(runs.len(), 6);
}

#[test]
fn append_extends_trailing_run_in_place_and_relocates_others() {
    let mut runs = RunVec::default();
    let r0 = parsed_run(&mut runs, &[10, 11]);
    let r1 = parsed_run(&mut runs, &[20]);

    // Trailing run extends in place.
    assert_eq!(runs.append(r1, 21), r1);
    assert_eq!(collect(&runs, r1), vec![20, 21]);

    // Duplicate append is a no-op even for a non-trailing run.
    assert_eq!(runs.append(r0, 11), r0);

    // Non-trailing run relocates to the arena end; old slots become garbage.
    let moved = runs.append(r0, 12);
    assert_ne!(moved, r0);
    assert_eq!(collect(&runs, moved), vec![10, 11, 12]);
    // The untouched run still reads correctly despite the garbage before it.
    assert_eq!(collect(&runs, r1), vec![20, 21]);
}

#[test]
fn remove_shifts_in_place_and_reports_emptied() {
    let mut runs = RunVec::default();
    let r0 = parsed_run(&mut runs, &[1, 2, 3]);
    let r1 = parsed_run(&mut runs, &[7]);

    assert_eq!(runs.remove(r0, &[9]), (0, false), "absent value: no-op");
    assert_eq!(runs.remove(r0, &[2]), (1, false));
    assert_eq!(collect(&runs, r0), vec![1, 3]);

    assert_eq!(runs.remove(r0, &[1, 3]), (2, true), "run emptied");
    // The neighbouring run is untouched by the in-place shrink.
    assert_eq!(collect(&runs, r1), vec![7]);
}

#[test]
fn arena_cap_is_enforced_exactly() {
    // Fill the arena to two slots under the cap, keeping the trailing-terminator invariant.
    let mut arena = vec![0; ARENA_CAP - 3];
    arena.push(TERMINATOR);
    let mut runs = RunVec { arena };

    // One more 2-slot run fits exactly at the cap…
    let last = runs.try_new_run(1).expect("2 free slots: must fit");
    assert_eq!(runs.len(), ARENA_CAP);

    // …after which value pushes and new runs are refused, leaving the arena untouched.
    assert!(!runs.try_append_last(last, 2));
    assert_eq!(runs.try_new_run(3), None);
    assert_eq!(runs.len(), ARENA_CAP);

    // Run starts always stay below the node-slot sentinel.
    assert!(last.as_usize() <= 0xFFFE);
}

#[test]
fn rebuild_drops_garbage_and_unmarked_runs() {
    let mut runs = RunVec::default();
    let r0 = parsed_run(&mut runs, &[0, 4]); // values: syms a(0), e(4)
    let r1 = parsed_run(&mut runs, &[2]); // c(2)
    let r2 = parsed_run(&mut runs, &[3]); // d(3) — never marked (dropped)
    let r0 = runs.append(r0, 1); // relocates r0, stranding garbage; adds b(1)

    let mut rb = RunRebuilder::new(runs.len(), 5);
    rb.mark_run_used(&runs, r0);
    rb.mark_run_used(&runs, r1);
    let rebuilt = rb.build(&runs);

    // Used values 0,1,2,4 get dense ids 0,1,2,3; the dropped run's value 3 gets none.
    assert_eq!(rebuilt.value_reidx[0], Some(0));
    assert_eq!(rebuilt.value_reidx[1], Some(1));
    assert_eq!(rebuilt.value_reidx[2], Some(2));
    assert_eq!(rebuilt.value_reidx[3], None);
    assert_eq!(rebuilt.value_reidx[4], Some(3));

    // New arena holds exactly the two marked runs (4 values + 2 terminators), no garbage.
    assert_eq!(rebuilt.runs.len(), 6);
    let new_r1 = RunIndex::from(rebuilt.runs_reidx[r1.as_usize()].unwrap());
    let new_r0 = RunIndex::from(rebuilt.runs_reidx[r0.as_usize()].unwrap());
    assert_eq!(rebuilt.runs_reidx[r2.as_usize()], None);
    // Values are remapped through the dense ids, order preserved.
    assert_eq!(collect(&rebuilt.runs, new_r0), vec![0, 3, 1]); // a,e,b
    assert_eq!(collect(&rebuilt.runs, new_r1), vec![2]); // c
}

#[test]
fn archived_round_trip() {
    let mut runs = RunVec::default();
    let r0 = parsed_run(&mut runs, &[3, 1]);
    let r1 = parsed_run(&mut runs, &[2]);

    let bytes = rkyv::to_bytes::<Error>(&runs).unwrap();
    let archived = rkyv::access::<super::ArchivedRunVec, Error>(&bytes[..]).unwrap();
    let view = archived.view();

    assert_eq!(view.run_at(r0).collect::<Vec<_>>(), vec![3, 1]);
    assert_eq!(view.run_at(r1).collect::<Vec<_>>(), vec![2]);

    // The externally-held cursor walks a run and parks at None.
    let mut cursor = Some(r0.as_u16());
    assert_eq!(view.next_in_run(&mut cursor), Some(3));
    assert_eq!(view.next_in_run(&mut cursor), Some(1));
    assert_eq!(view.next_in_run(&mut cursor), None);
    assert_eq!(cursor, None);
    assert_eq!(view.next_in_run(&mut cursor), None, "stays parked");
}

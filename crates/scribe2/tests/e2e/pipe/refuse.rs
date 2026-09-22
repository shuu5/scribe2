// flip-check: moved s2-07l.351
//! 断りの語彙の歯: `pipe_refuse_`（write-set の排他と `stop --run`）。
//!
//! 共有 helper は親（`tests/e2e/pipe.rs`）に在り `use super::*` で引き、受付の口の歯と共有する helper は
//! `use super::intake::{…}` で引く（歯の本文は `intake.rs` から移しただけ・`s2-07l.351`）。

use super::*;
use super::intake::{record_stage, write_verdict};

/// live な便（intake だけ通した段 `Intake`）と **write-set が交差する 2 本目**は受け付けない
/// （ADR-0019 §2.1）。断った周は run dir も event も作らず、stderr が 1 本目の run id と
/// 交差した path を名乗る。**base はこの 2 本目を受理する**（run dir が 2 つできる）。
#[test]
fn pipe_refuse_intake_refuses_a_contract_that_overlaps_a_live_run() {
    let (repo, state) = repo_with_state();
    let first = write_set_contract(&repo, "first", &["src/lib.rs"]);
    let id = intake_bead(&repo, &state, &first, "s2-live");
    let before = events_bytes(&state);
    let dirs = run_dirs(&state);
    let second = write_set_contract(&repo, "second", &["src/lib.rs"]);
    let out = try_intake(&repo, &state, &second, "s2-next");
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "交差は rc 1: {}", stdout_of(&out));
    let err = stderr_of(&out);
    assert!(err.contains(&id), "1 本目の run id を名乗る: {err}");
    assert!(err.contains("src/lib.rs"), "交差した path を名乗る: {err}");
    assert!(out.stdout.is_empty(), "断った周は stdout に 1 byte も書かない");
    assert_eq!(run_dirs(&state), dirs, "run dir を作らない（母集団 {} 本）", dirs.len());
    assert_eq!(events_bytes(&state), before, "events.jsonl は byte 不変");
    clean(&[&repo, &state]);
}

/// dir と file の交差の表（設計 §2）を **intake の受理 / 拒否**で測る。正規化は write-set
/// guard と同じ規則（先頭の `./`・連続する `/`・`..` の畳み）で、dir `src/` は `src/…` を含み
/// `srcx/` は含まない。dir 項目は base の tracked file に**展開して**数える（契約 (g)・設計 contract-source.md §3）
/// ＝base に在る `src/lib.rs` は `src/` と交差し、新規 file（`+src/new.rs`）と base に無い file は交差しない。
#[test]
fn pipe_refuse_intake_measures_dir_and_file_overlap() {
    for (live_entry, next_entry, refused) in [
        ("src/", "src/lib.rs", true),
        ("src/", "srcx/", false),
        ("src/", "src/", true),
        // 正規化していない形（`./x` / `x//y` / `x/../x`）は**行が持てない**（表の検査が base に解けないと断る）。
        // 畳み方そのものは `pipe::refuse` の in-file の歯（`normalize` / `overlaps`）が測る。
        ("src/", "+src/new.rs", false),
        // base に無い file を**素の path**で持つ行は表の検査が断る（`+` を付けるのが行の形）＝
        // 「dir と未来の file は交差しない」は上の `+` の対で測る。
        ("+src/new.rs", "src/new.rs", true),
    ] {
        let (repo, state) = repo_with_state();
        let first = write_set_contract(&repo, "first", &[live_entry]);
        intake_bead(&repo, &state, &first, "s2-live");
        let second = write_set_contract(&repo, "second", &[next_entry]);
        let out = try_intake(&repo, &state, &second, "s2-next");
        let want = if refused { RC_REFUSED } else { RC_OK };
        assert_eq!(
            out.status.code(),
            Some(i32::from(want)),
            "{live_entry} × {next_entry} は交差={refused}: {}",
            stderr_of(&out)
        );
        clean(&[&repo, &state]);
    }
}

/// **終端した便とは交差しない**（段が `Landed` / `Failed` / `Stopped`・`Gated` で verdict が
/// FAIL）。`Gated` の PASS / INCONCLUSIVE は終端でないので交差する（pipeline.md §4「FAIL は終端」）。
/// 契約を改訂して流し直す経路（本番 `.129` / `.131` の型）を塞がないことを測る。
#[test]
fn pipe_refuse_intake_ignores_terminal_runs() {
    for (stage, verdict, refused) in [
        ("Landed", None, false),
        ("Failed", None, false),
        ("Stopped", None, false),
        ("Gated", Some("FAIL"), false),
        ("Gated", Some("PASS"), true),
        ("Gated", Some("INCONCLUSIVE"), true),
    ] {
        let (repo, state) = repo_with_state();
        let first = write_set_contract(&repo, "first", &["src/lib.rs"]);
        let id = intake_bead(&repo, &state, &first, "s2-live");
        if let Some(found) = verdict {
            write_verdict(&state, &id, found);
        }
        record_stage(&state, &id, stage);
        let second = write_set_contract(&repo, "second", &["src/lib.rs"]);
        let out = try_intake(&repo, &state, &second, "s2-next");
        let want = if refused { RC_REFUSED } else { RC_OK };
        assert_eq!(
            out.status.code(),
            Some(i32::from(want)),
            "段 {stage} verdict {verdict:?} は交差={refused}: {}",
            stderr_of(&out)
        );
        clean(&[&repo, &state]);
    }
}

/// live な便の契約の写しを読めない周は **rc 2**（壊れた store・NFR4）で、run dir も event も
/// 作らない。`Gated` の判定を読めない周も同じ「読めない」側である（fail-closed＝読めなさを
/// 「交差なし」に読み替えない）。
#[test]
fn pipe_refuse_intake_is_broken_when_a_live_copy_is_unreadable() {
    for damage in ["remove", "garble", "verdict"] {
        let (repo, state) = repo_with_state();
        let first = write_set_contract(&repo, "first", &["src/lib.rs"]);
        let id = intake_bead(&repo, &state, &first, "s2-live");
        let copied = state.join("pipe").join(&id).join("contract.toml");
        match damage {
            "remove" => {
                fs::remove_file(&copied).ok();
            }
            "garble" => {
                fs::write(&copied, "こわれ\n").ok();
            }
            // 段は Gated だが verdict.json が無い＝終端かを測れない。
            _ => record_stage(&state, &id, "Gated"),
        }
        let before = events_bytes(&state);
        let dirs = run_dirs(&state);
        let second = write_set_contract(&repo, "second", &["src/lib.rs"]);
        let out = try_intake(&repo, &state, &second, "s2-next");
        assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "{damage}: 読めない周は rc 2");
        let err = stderr_of(&out);
        assert!(err.contains(&id), "{damage}: 読めない run を名指す: {err}");
        assert!(err.contains("読めない"), "{damage}: 理由は読めないこと: {err}");
        assert_eq!(run_dirs(&state), dirs, "{damage}: run dir を作らない");
        assert_eq!(events_bytes(&state), before, "{damage}: events.jsonl は byte 不変");
        clean(&[&repo, &state]);
    }
}

/// **同じ bead の 2 本目も特別扱いしない**: write-set が同じなら交差で断られる（owner が同じ
/// ことに意味を持たせない＝自然に掛かる）。
#[test]
fn pipe_refuse_intake_refuses_the_second_run_of_the_same_bead() {
    let (repo, state) = repo_with_state();
    let path = write_set_contract(&repo, "first", &["src/lib.rs"]);
    let id = intake_bead(&repo, &state, &path, "s2-same");
    let out = try_intake(&repo, &state, &path, "s2-same");
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "同 bead の 2 本目も断る");
    let err = stderr_of(&out);
    assert!(err.contains("交差"), "断る理由は id の衝突でなく交差: {err}");
    assert!(err.contains(&id), "交差した相手を名乗る: {err}");
    clean(&[&repo, &state]);
}

/// 交差が 2 組以上の周は **stderr に全組が 1 組 1 行**で並び、理由の 1 行は先頭の 1 組を名乗る。
#[test]
fn pipe_refuse_intake_lists_every_overlapping_pair() {
    let (repo, state) = repo_with_state();
    let first = write_set_contract(&repo, "first", &["src/a.rs", "src/b.rs"]);
    let id = intake_bead(&repo, &state, &first, "s2-live");
    let second = write_set_contract(&repo, "second", &["src/a.rs", "src/b.rs"]);
    let out = try_intake(&repo, &state, &second, "s2-next");
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "交差は rc 1");
    let err = stderr_of(&out);
    let pairs: Vec<&str> = err.lines().filter(|line| line.contains("overlap ")).collect();
    assert_eq!(pairs.len(), 2, "交差した全組が並ぶ（母集団 {} 行）: {err}", err.lines().count());
    for entry in ["src/a.rs", "src/b.rs"] {
        assert!(
            pairs.iter().any(|line| line.contains(entry) && line.contains(&id)),
            "{entry} の組が run id つきで並ぶ: {err}"
        );
    }
    let head = err.lines().next().unwrap_or_default();
    assert!(head.contains("src/a.rs"), "理由の 1 行は先頭の 1 組: {head}");
    assert!(!head.contains("src/b.rs"), "理由の 1 行は 1 組だけ: {head}");
    clean(&[&repo, &state]);
}

/// `pipe stop --run <id>`: 終端でない便 1 本に `RunStopped` を **1 件だけ**書き、その後は同じ
/// write-set の契約が通る。終端した便には何も書かず rc 1（書込は冪等・rc は冪等でない）。
#[test]
fn pipe_refuse_stop_run_releases_the_write_set_of_a_live_run() {
    let (repo, state) = repo_with_state();
    let first = write_set_contract(&repo, "first", &["src/lib.rs"]);
    let id = intake_bead(&repo, &state, &first, "s2-live");
    let second = write_set_contract(&repo, "second", &["src/lib.rs"]);
    let blocked = try_intake(&repo, &state, &second, "s2-next");
    assert_eq!(blocked.status.code(), Some(i32::from(RC_REFUSED)), "止める前は交差で断られる");
    let before = event_count(&state);
    let out = run_pipe(&["stop", "--run", &id, "--state-dir", &state.display().to_string()]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "非終端の便は止まる: {}", stderr_of(&out));
    assert_eq!(event_count(&state), before + 1, "RunStopped を 1 件だけ書く");
    let last = events(&state).into_iter().rfind(|found| found.run == id);
    assert!(
        matches!(&last, Some(found) if found.kind == EventKind::RunStopped && found.stage == Some(Stage::Stopped)),
        "書くのは RunStopped stage=Stopped: {last:?}"
    );
    // 2 回撃っても 2 件目を書かない（終端した便は rc 1）。
    let again = run_pipe(&["stop", "--run", &id, "--state-dir", &state.display().to_string()]);
    assert_eq!(again.status.code(), Some(i32::from(RC_REFUSED)), "終端の便は rc 1");
    assert_eq!(event_count(&state), before + 1, "2 件目を書かない");
    // 外れた便とは交差しない＝同じ write-set の契約が通る。
    let passed = try_intake(&repo, &state, &second, "s2-third");
    assert_eq!(passed.status.code(), Some(i32::from(RC_OK)), "止めた後は通る: {}", stderr_of(&passed));
    // 無い便は rc 1 で何も書かない。
    let missing = run_pipe(&["stop", "--run", "no-such-run", "--state-dir", &state.display().to_string()]);
    assert_eq!(missing.status.code(), Some(i32::from(RC_REFUSED)), "無い便は rc 1");
    clean(&[&repo, &state]);
}

/// `stop --run` は便の **Live 席も止める**（`--all` と同じ関数を通る）。席を持つ便を外す口が
/// 席を残すと、止めたはずの便の runner が走り続ける。
#[test]
fn pipe_refuse_stop_run_stops_the_live_seat_of_the_run() {
    let (repo, state) = repo_with_state();
    let path = write_set_contract(&repo, "first", &["src/lib.rs"]);
    let id = intake_bead(&repo, &state, &path, "s2-live");
    // **孫**として起こす（test process の子のままだと zombie が /proc に残る）。
    let spawned = Command::new("sh")
        .arg("-c")
        .arg("sleep 60 >/dev/null 2>&1 & echo $!")
        .output()
        .expect("fake runner を起こせる");
    let pid: u32 = String::from_utf8_lossy(&spawned.stdout).trim().parse().expect("pid を読める");
    let record = bin_cmd()
        .args(["fleet", "record", "--kind", "SeatSpawned", "--run", &id, "--bead", "s2-live",
               "--seat", "seat-1", "--pid", &pid.to_string(), "--state-dir"])
        .arg(&state)
        .output()
        .expect("binary を起動できる");
    assert_eq!(record.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&record));
    let out = run_pipe(&["stop", "--run", &id, "--state-dir", &state.display().to_string()]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "席ごと止まる: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("seats=1 stopped=1"), "席を数える: {}", stdout_of(&out));
    assert!(!Path::new(&format!("/proc/{pid}")).exists(), "runner の process は消えている");
    let kinds: Vec<EventKind> = events(&state)
        .into_iter()
        .filter(|found| found.run == id)
        .map(|found| found.kind)
        .collect();
    assert!(kinds.contains(&EventKind::SeatStopped), "席にも記帳する: {kinds:?}");
    assert!(kinds.contains(&EventKind::RunStopped), "便にも記帳する: {kinds:?}");
    clean(&[&repo, &state]);
}

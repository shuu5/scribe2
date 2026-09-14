//! 管理 tick の歯（`seat tick` / `seat heartbeat` / `seat meter`・作り直しと送達の証拠・
//! 設計 docs/design/seat-autonomy.md §3・接頭辞 `seat_tick_` / `seat_heartbeat_` / `seat_meter_` / `seat_evidence_`）。
//!
//! 共有の helper と fixture は親 module（`tests/e2e/seat.rs`）に在り、`use super::*` で使う。
//! 歯の本文は `seat.rs` から**挙動不変で移した**もの（`s2-07l.261`）。
// flip-check: moved s2-07l.261

use super::*;

/// `seat meter` を capture-file 経由で 1 回撃つ（tmux を呼ばない経路）。
fn meter_on(path: &str, transcript: Option<&str>) -> Output {
    let mut args = vec!["meter", "--target", "unused", "--capture-file", path];
    if let Some(found) = transcript {
        args.push("--transcript");
        args.push(found);
    }
    run_seat(&args)
}

// ─────────────────────────── meter ───────────────────────────

/// statusline は **最後の prompt 行より下**から採る（上の decoy は候補にしない）。
#[test]
fn seat_meter_parses_statusline_below_prompt_anchor() {
    let dir = tmp();
    let pane = "99% 1k/2k Fable\n❯ \n  45% 90k/200k Opus 5 [high]\n";
    let path = fixture(&dir, "pane.txt", pane);
    let out = meter_on(&path, None);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        "seat: meter used_pct=45 used_tokens=90000 window_tokens=200000 source=pane\n"
    );
    assert_eq!(stderr_of(&out), "", "成立した周は stderr へ 1 byte も書かない");

    // anchor の負例: 有効な statusline が **prompt より上にしか無い** pane は採らない
    // （「後ろから探す」だけの実装だと、この pane から 45% を拾ってしまう）。
    let above = fixture(&dir, "above.txt", "  45% 90k/200k Opus 5 [high]\n❯ \n");
    let out = meter_on(&above, None);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "prompt より上の statusline は採らない");
    assert_eq!(stdout_of(&out), "");
    assert_eq!(stderr_of(&out), "seat: meter unmeasured reason=no-source\n");

    // 健全性の**内側の境界**（pct = 100 ちょうど・used == window・M 倍率）は採る。
    let edge = fixture(&dir, "edge.txt", "❯ \n  100% 1M/1M Opus 5\n");
    let out = meter_on(&edge, None);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        "seat: meter used_pct=100 used_tokens=1000000 window_tokens=1000000 source=pane\n"
    );

    // window の下限ちょうど（100000）も採る。
    let floor = fixture(&dir, "floor.txt", "❯ \n  1% 1k/100k Opus 5\n");
    let out = meter_on(&floor, None);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        "seat: meter used_pct=1 used_tokens=1000 window_tokens=100000 source=pane\n"
    );
    fs::remove_dir_all(&dir).ok();
}

/// 健全性（pct ≤ 100 ∧ used ≤ window ∧ window ≥ 100000）を外れた値は流さない。
#[test]
fn seat_meter_rejects_out_of_bound_statusline() {
    let dir = tmp();
    let cases = [
        ("pct.txt", "❯ \n  120% 90k/200k\n"),
        ("used.txt", "❯ \n  45% 300k/200k\n"),
        ("window.txt", "❯ \n  45% 10k/50k\n"),
    ];
    for (name, body) in cases {
        let path = fixture(&dir, name, body);
        let out = meter_on(&path, None);
        assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{name}");
        assert_eq!(stdout_of(&out), "", "{name}: 不成立の周は stdout 0 行");
        assert_eq!(
            stderr_of(&out),
            "seat: meter unmeasured reason=pane-out-of-bound\n",
            "{name}"
        );
    }
    fs::remove_dir_all(&dir).ok();
}

/// **transcript が名指された周は transcript を見る**（健全な statusline が在っても）。
///
/// 出所を入力で決める 1 本道にするための歯である——pane 一次のままだと、hook の中で pane を
/// 持てない guard（C2.2）と meter が同じ席の同じ瞬間に違う値を返す。値は最後の有効な usage で、
/// decoy 3 種（sidechain / usage null / 和 0）はいずれも数えない。
#[test]
fn seat_meter_reads_the_named_transcript_over_the_pane() {
    let dir = tmp();
    // pane 側は**健全な statusline**（90%）。transcript が勝つので、この値は出ない。
    let pane = fixture(&dir, "pane.txt", "❯ \n  90% 900k/1M Opus 5\n");
    let jsonl = concat!(
        r#"{"type":"assistant","isSidechain":false,"message":{"usage":{"input_tokens":200,"cache_creation_input_tokens":300,"cache_read_input_tokens":6500}}}"#,
        "\n",
        r#"{"type":"assistant","isSidechain":true,"message":{"usage":{"input_tokens":999999,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#,
        "\n",
        r#"{"type":"assistant","isSidechain":false,"message":{"usage":null}}"#,
        "\n",
        r#"{"type":"assistant","isSidechain":false,"message":{"usage":{"input_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#,
        "\n",
        // **最後の**有効 entry（これが採られる＝先頭の 7000 ではない）。宣言窓 1000000 に対して
        // 250000 = 25% で、**使用率が 0 でも 100 でもない**値になる形にしてある（0% は
        // 「割っていない実装」でも通ってしまう）。
        r#"{"type":"assistant","isSidechain":false,"message":{"usage":{"input_tokens":30000,"cache_creation_input_tokens":70000,"cache_read_input_tokens":150000}}}"#,
        "\n",
    );
    let transcript = fixture(&dir, "transcript.jsonl", jsonl);
    let out = meter_on(&pane, Some(&transcript));

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        "seat: meter used_pct=25 used_tokens=250000 window_tokens=1000000 source=jsonl+rules\n",
        "最後の有効な和を宣言窓で割った実値が載る（pane の 90% ではない）"
    );
    fs::remove_dir_all(&dir).ok();
}

/// **空の `--transcript` は「渡していない」と同じ**（trim 後）。
///
/// 空の口をそのまま path として扱うと、渡し忘れが `unreadable`（file が壊れている）に化け、
/// 健全な pane が在るのに不成立になる＝記録から原因を取り違える。
#[test]
fn seat_meter_treats_empty_transcript_as_absent() {
    let dir = tmp();
    let pane = fixture(&dir, "pane.txt", "❯ \n  90% 900k/1M Opus 5\n");
    let out = meter_on(&pane, Some("   "));
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        "seat: meter used_pct=90 used_tokens=900000 window_tokens=1000000 source=pane\n",
        "空の口は渡し忘れと同じ＝pane を読む（unreadable に化けない）"
    );
    fs::remove_dir_all(&dir).ok();
}

/// transcript が名指されたのに測れない周は、**guard の記録と同じ語**で不成立になる。
///
/// 同じ条件を 2 面が別の語で呼ぶと（本便より前の `jsonl-no-usage` と `no-usage`）、記録と
/// CLI を突き合わせたときに同じ事象が別物に見える。語は 2 段を畳まない（file が読めないのか、
/// 有効な usage が無いのか）。
#[test]
fn seat_meter_names_the_same_unmeasured_reasons_as_the_guard() {
    let dir = tmp();
    let pane = fixture(&dir, "pane.txt", "❯ \n  90% 900k/1M Opus 5\n");
    let empty = fixture(&dir, "empty.jsonl", "");
    for (transcript, reason) in [(format!("{empty}-nope"), "unreadable"), (empty.clone(), "no-usage")] {
        let out = meter_on(&pane, Some(&transcript));
        assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "測れない周は rc≠0: {reason}");
        assert_eq!(stdout_of(&out), "", "不成立に stdout は出さない: {reason}");
        assert_eq!(
            stderr_of(&out),
            format!("seat: meter unmeasured reason={reason}\n"),
            "guard の記録と同じ語で名乗る（pane の 90% へ逃げない）"
        );
    }
    fs::remove_dir_all(&dir).ok();
}

/// 窓の宣言が**不発効**か**0** なら窓を引けない（＝割らずに不成立へ倒す側）。
///
/// 埋め込みの manifest では起こらないが、3 つの述語（行の有無 / `enabled` / `> 0`）を測る口が
/// 無いと、どれを外しても歯が落ちない。`Manifest::parse` へ fixture を渡して動かす
/// （`tests/e2e/fleet.rs` の `LockPolicy::from_rules` と同型）。
///
/// `enabled` は**必須 key**なので、発効側の fixture も `enabled = true` を明記する
/// （`s2-07l.80`）。この便の test 区間の差はその字面だけで、assert の意味は 1 つも
/// 動かない——base の loader は書いた行も同じ値で読むので、base で新しく赤くなる歯は無い。
// flip-check: retroactive s2-07l.80
#[test]
fn seat_meter_refuses_a_window_row_that_is_off_or_zero() {
    let row = |extra: &str, value: u64| {
        format!(
            "schema = 1\n\n[[rule]]\nid = \"seat.context_window_tokens\"\nkind = \"SeatContextWindowTokens\"\nvalue = {value}\n{extra}ruling = \"r\"\nruled_at = \"d\"\n"
        )
    };
    let live = Manifest::parse(&row("enabled = true\n", 1_000_000)).expect("fixture を読める");
    assert_eq!(window_of(&live), Some(1_000_000), "発効した正の行は引ける");
    let off = Manifest::parse(&row("enabled = false\n", 1_000_000)).expect("fixture を読める");
    assert_eq!(window_of(&off), None, "不発効の行は引かない（値は在っても使わない）");
    let zero = Manifest::parse(&row("enabled = true\n", 0)).expect("fixture を読める");
    assert_eq!(window_of(&zero), None, "0 は引かない（0 で割らない）");
    let absent = Manifest::parse("schema = 1\n").expect("fixture を読める");
    assert_eq!(window_of(&absent), None, "行そのものが無い周も引かない");
}

/// cap は guard と管理 tick が**同じ関数**で読む（`s2-07l.89`）: 発効した行は引き、不発効・
/// 型違い・行不在は `None`（測らない側）。**0 は引く**（「常に止める」の宣言であって欠落では
/// ない・窓の `> 0` とは別物）。tick 側の境界の歯（60 / 59）と対で「1 本の口」を測る。
#[test]
fn seat_meter_reads_cap_from_the_manifest_row() {
    let row = |extra: &str, value: &str| {
        format!(
            "schema = 1\n\n[[rule]]\nid = \"seat.context_cap_pct\"\nkind = \"SeatContextCapPct\"\nvalue = {value}\n{extra}ruling = \"r\"\nruled_at = \"d\"\n"
        )
    };
    let live = Manifest::parse(&row("enabled = true\n", "60")).expect("fixture を読める");
    assert_eq!(cap_of(&live), Some(60), "発効した行は引ける");
    let off = Manifest::parse(&row("enabled = false\n", "60")).expect("fixture を読める");
    assert_eq!(cap_of(&off), None, "不発効の行は引かない（値は在っても使わない）");
    let zero = Manifest::parse(&row("enabled = true\n", "0")).expect("fixture を読める");
    assert_eq!(cap_of(&zero), Some(0), "0 は引く（常に止める宣言・欠落ではない）");
    let absent = Manifest::parse("schema = 1\n").expect("fixture を読める");
    assert_eq!(cap_of(&absent), None, "行そのものが無い周は引かない");
}

/// 出所が 1 つも成立しない周は理由つきで不成立になる（0% に化けない）。
///
/// 契約の pin は (a)。(b) は同じ「statusline 無し」でも **pane 本文は在る**周で、
/// 理由語を `no-source` と弁別する（読みの SSOT = 設計 §3 の箇条）。
#[test]
fn seat_meter_unmeasured_without_any_source() {
    let dir = tmp();
    let empty = fixture(&dir, "empty.txt", "");
    let out = meter_on(&empty, None);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED));
    assert_eq!(stdout_of(&out), "");
    assert_eq!(stderr_of(&out), "seat: meter unmeasured reason=no-source\n");

    let noisy = fixture(&dir, "noisy.txt", "❯ \n  building…\n  ready\n");
    let out = meter_on(&noisy, None);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED));
    assert_eq!(stdout_of(&out), "");
    assert_eq!(
        stderr_of(&out),
        "seat: meter unmeasured reason=pane-no-statusline\n"
    );
    fs::remove_dir_all(&dir).ok();
}

// ─────────────────── heartbeat / tick ───────────────────

/// prompt より下が空＝出所なし。
const CTX_NO_SOURCE: &str = " context=unmeasured reason=no-source";
/// cap（manifest 行 `seat.context_cap_pct` = 60）**以上**の席の pane（インシデントの形: lens 待ちの
/// まま 96%・spinner の字面が prompt の上）。busy は打刻で与える。両側から撃つ＝manifest の値が変わると落ちる。
const OVER_CAP_BUSY_PANE: &str = concat!(
    "✻ Sublimating… (19m 36s · ↓ 36.4k tokens)\n",
    "────────────────────────────────────────\n",
    "❯\u{a0}\n",
    "────────────────────────────────────────\n",
    "  user@host (user@example.com)  scribe2  main\n",
    "  96% 960k/1M Fable 5.1 [high] 5h:10%(4h22m) 7d:15%(6d8h)\n",
    "  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← 1 agent\n",
);
/// cap 以上で turn を終えた字面の席（退避済みの席が `/clear` を待つ形）。
const OVER_CAP_IDLE_PANE: &str = concat!(
    "✻ Crunched for 10m 28s · done 12:41\n",
    "────────────────────────────────────────\n",
    "❯\u{a0}\n",
    "────────────────────────────────────────\n",
    "  user@host (user@example.com)  scribe2  main\n",
    "  96% 960k/1M Fable 5.1 [high] 5h:10%(4h22m) 7d:15%(6d8h)\n",
    "  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← 1 agent\n",
);
/// cap **未満**で spinner の字面を持つ席。
const BELOW_CAP_BUSY_PANE: &str = concat!(
    "✻ Sublimating… (2m 3s · ↓ 4.1k tokens)\n",
    "────────────────────────────────────────\n",
    "❯\u{a0}\n",
    "────────────────────────────────────────\n",
    "  user@host (user@example.com)  scribe2  main\n",
    "  12% 120k/1M Fable 5.1 [high] 5h:10%(4h22m) 7d:15%(6d8h)\n",
    "  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← 1 agent\n",
);
/// statusline を持たない idle の席（prompt より下に候補でない行だけ）。
const NO_STATUSLINE_IDLE_PANE: &str = "❯ \n  ⏵⏵ bypass permissions on (shift+tab to cycle)\n";
/// statusline の候補は在るが健全性を外れた idle の席（pct > 100）。
const OUT_OF_BOUND_IDLE_PANE: &str = "❯ \n  150% 1500k/1M Opus 5\n";

/// 打刻は無ければ作り、2 回目で mtime が進む。空の `--state-dir` は使い方の誤りとして断る。
#[test]
fn seat_heartbeat_touches_seat_file() {
    let dir = tmp();
    let state = dir.join("state");
    let state_s = state.display().to_string();
    // `:` と `.` を含む実際の target の形で撃つ（置き場の名前は潰した字面になる）。
    let marker = seat_dir_of(&state, "seatbeat_0.0").join("heartbeat");

    let out = run_seat(&["heartbeat", "--target", "seatbeat:0.0", "--state-dir", &state_s]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!("seat: heartbeat target=seatbeat_0.0{}\n", provenance(&state, "flag")),
        "成功行は打刻先と、その解決の出所（--state-dir）を持つ"
    );
    assert_eq!(stderr_of(&out), "", "成立した周は stderr へ 1 byte も書かない");
    assert!(marker.exists(), "打刻 file が在る");
    // A/B: 行が名乗る置き場から組んだ marker が、実際に書かれた marker と 1 対 1 で一致する。
    let claimed = state_dir_in(stdout_of(&out).trim_end()).map(|d| seat_dir_of(&d, "seatbeat_0.0").join("heartbeat"));
    assert_eq!(claimed.as_deref(), Some(marker.as_path()), "行の path と実体の親 dir が一致する");

    backdate(&marker, 600);
    let before = mtime_of(&marker);
    let out = run_seat(&["heartbeat", "--target", "seatbeat:0.0", "--state-dir", &state_s]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert!(mtime_of(&marker) > before, "2 回目で mtime が進む（開くだけでは進まない）");

    // 空の置き場は cwd 相対に化けるので、渡し忘れとして断る（1 byte も書かない）。
    let out = run_seat_in(&dir, &["heartbeat", "--target", "empt", "--state-dir", ""]);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED));
    assert!(stderr_of(&out).starts_with("usage: seat "), "{}", stderr_of(&out));
    assert!(!dir.join("seat").exists(), "cwd に置き場を作らない");
    fs::remove_dir_all(&dir).ok();
}

/// 相対の `--state-dir` は **cwd で絶対化した path** を名乗る（flag の字面の echo ではない）。
/// 相対のままでは cwd に依存して「どこへ」を名乗れず、行が実体と 1 対 1 にならない。
/// 空白と ` source=` を含む dir 名でも、出所が先・path が行末なので出所を偽れない。
#[test]
fn seat_heartbeat_names_absolute_state_dir_for_relative_flag() {
    let dir = tmp();
    let cwd = dir.join("sub");
    fs::create_dir_all(&cwd).ok();
    let rel = "rel state source=git-config";
    let state = cwd.join(rel);
    let marker = seat_dir_of(&state, "seatrel").join("heartbeat");

    let out = run_seat_in(&cwd, &["heartbeat", "--target", "seatrel", "--state-dir", rel]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!("seat: heartbeat target=seatrel{}\n", provenance(&state, "flag")),
        "相対の flag は cwd で絶対化した path を名乗り、出所は flag のまま"
    );
    assert!(marker.exists(), "打刻は cwd 相対の実体に落ちる");
    // A/B: 行が名乗る path（flag の字面ではない）から組んだ marker が実体と 1 対 1 で一致する。
    let claimed = state_dir_in(stdout_of(&out).trim_end()).map(|d| seat_dir_of(&d, "seatrel").join("heartbeat"));
    assert_eq!(claimed.as_deref(), Some(marker.as_path()), "行の path と実体の親 dir が一致する");
    assert!(
        !stdout_of(&out).contains(&format!(" state_dir={rel}")),
        "flag の字面をそのまま echo しない: {}",
        stdout_of(&out)
    );
    fs::remove_dir_all(&dir).ok();
}

/// `--state-dir` が無い周は **tmp repo** の git 設定から置き場を解き、行と記録に
/// `source=git-config` と解いた path を出す（anchor の設定は触らない）。
///
/// 出所の事故（2026-09-10・planner 席）: 設定が死んだ dir を指していても rc 0 の成功行が出る。
/// 席側の打刻行と tick 側の記録を並べるだけで別の dir を見ていると分かる形にする。
#[test]
fn seat_heartbeat_and_tick_resolve_state_dir_from_git_config() {
    let dir = tmp();
    let repo = dir.join("repo");
    let state = dir.join("state-from-config");
    fs::create_dir_all(&repo).ok();
    let init = Command::new("git").args(["-C"]).arg(&repo).args(["init", "-q"]).output();
    assert!(init.is_ok_and(|out| out.status.success()), "tmp repo を作れる");
    let set = Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["config", &format!("{NAME}.stateDir")])
        .arg(&state)
        .output();
    assert!(set.is_ok_and(|out| out.status.success()), "tmp repo に置き場を設定できる");
    let marker = seat_dir_of(&state, "seatgit").join("heartbeat");

    let out = run_seat_in(&repo, &["heartbeat", "--target", "seatgit"]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!("seat: heartbeat target=seatgit{}\n", provenance(&state, "git-config")),
        "git 設定から解いた周は source=git-config"
    );
    assert!(marker.exists(), "打刻は設定が指す dir に落ちる");
    let claimed = state_dir_in(stdout_of(&out).trim_end()).map(|d| seat_dir_of(&d, "seatgit").join("heartbeat"));
    assert_eq!(claimed.as_deref(), Some(marker.as_path()), "行の path と実体の親 dir が一致する");

    // 同じ設定から解く tick は同じ dir を読む（直前の打刻は判定入力ではない・`s2-07l.109`）: 打刻 file が
    // 無く pane も無い席は `pane-missing`（状態の列は missing）。記録にも同じ 2 語が載る。
    let (wm_s, sock_s) = (dir.join("wm").display().to_string(), dir.join("absent-sock").display().to_string());
    let out = run_seat_in(&repo, &["tick", "--target", "seatgit", "--wm-dir", &wm_s, "--tmux-socket", &sock_s]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!("seat: tick decision=noop reason=pane-missing{ST_MISSING}{}\n", provenance(&state, "git-config"))
    );
    let recorded = fs::read_to_string(tick_file(&state, "seatgit")).unwrap_or_default();
    assert!(
        recorded.contains(&format!(
            r#""what":"decision=noop reason=pane-missing{ST_MISSING}{}""#,
            provenance(&state, "git-config")
        )),
        "記録の what にも置き場と出所が載る: {recorded}"
    );
    fs::remove_dir_all(&dir).ok();
}

/// 条件は**順序固定**で見て、最初に立たなかった条件を理由にする。
///
/// heartbeat の経過は両側から置く（`STALE_S - 1` / `STALE_S + 1`）が、どちらの組も同じ理由で
/// 止まる＝heartbeat の mtime が判定入力でないこと（`s2-07l.109`）も測れる。
#[test]
fn seat_tick_noop_reasons_in_fixed_order() {
    let target = "seatorder";
    let cases = fixed_order_cases(target);
    for (at, case) in cases.iter().enumerate() {
        let dir = tmp();
        let state = prepare_tick_case(&dir, case, target);
        let (wm_s, pane_s) = (
            dir.join("wm").display().to_string(),
            dir.join("pane.txt").display().to_string(),
        );
        let (sock_s, state_s) = (
            dir.join("absent-sock").display().to_string(),
            state.display().to_string(),
        );
        let mut args = vec![
            "tick", "--target", target, "--wm-dir", &wm_s, "--tmux-socket", &sock_s,
            "--state-dir", &state_s,
        ];
        if case.via_file {
            args.push("--capture-file");
            args.push(&pane_s);
        }
        let (out, touched) = run_seat_probed(&dir, &args);
        assert_tick_case(&out, touched, case, at, &state);
        let recorded = fs::read_to_string(tick_file(&state, target)).unwrap_or_default();
        assert_eq!(recorded.lines().count(), 1, "組 {at}: 記録は 1 行: {recorded}");
        assert!(recorded.contains(r#""who":"seat-tick""#), "組 {at}: {recorded}");
        assert!(
            recorded.contains(&format!(
                r#""what":"decision=noop reason={}{}{}{}""#,
                case.reason,
                case.context,
                case.state,
                provenance(&state, "flag")
            )),
            "組 {at}: 判定を残す（context と state の列と置き場の出所も記録に載る）: {recorded}"
        );
        fs::remove_dir_all(&dir).ok();
    }
}

/// 順序固定の歯の組（**先に立たない条件だけが違う**）。
fn fixed_order_cases(target: &'static str) -> Vec<TickCase> {
    vec![
        // heartbeat が fresh でも鮮度では止まらない（`s2-07l.109`・鮮度 gate 撤去）: 状態を読み、pane を
        // 取りに行く＝tmux に当たる。退避物は**他席**の名乗り（自席の申告ではない）。
        TickCase { reason: "pane-missing", beat_age_s: Some(STALE_S - 1), pane: None,
                   wm_seat: Some("other:seat"), via_file: false, tmux: true, context: "",
                   stamp: StateFix::Busy { age_s: 0 }, state: ST_BUSY },
        // pane より先に状態を読む＝pane-missing の行にも state の列が載る。
        TickCase { reason: "pane-missing", beat_age_s: None, pane: None,
                   wm_seat: Some(target), via_file: true, tmux: false, context: "",
                   stamp: StateFix::Idle, state: ST_IDLE },
        // heartbeat が stale でも同じ: pane を読みに行く＝tmux に当たる（fresh の組と同じ理由）。
        TickCase { reason: "pane-missing", beat_age_s: Some(STALE_S + 1), pane: None,
                   wm_seat: Some(target), via_file: false, tmux: true, context: "",
                   stamp: StateFix::Idle, state: ST_IDLE },
        // 状態の門は typed の打刻で決まる: 入力欄が空の字面でも Busy の打刻なら busy。
        // 壁時計では等号を pin しない（`age_s: STALE_S` は CI の 1 秒遅れで stale へ反転した・
        // `s2-07l.118`・main 9e2cb42 run 34619928421）＝閾値の内側は境界から離して置く。
        // flip-check: retroactive s2-07l.118
        TickCase { reason: "busy", beat_age_s: None, pane: Some(IDLE_PANE),
                   wm_seat: Some(target), via_file: true, tmux: false, context: CTX_10,
                   stamp: StateFix::Busy { age_s: STALE_S / 2 }, state: ST_BUSY },
        TickCase { reason: "state-missing", beat_age_s: None, pane: Some(IDLE_PANE),
                   wm_seat: Some(target), via_file: true, tmux: false, context: CTX_10,
                   stamp: StateFix::Absent, state: ST_MISSING },
        TickCase { reason: "state-unreadable", beat_age_s: None, pane: Some(IDLE_PANE),
                   wm_seat: Some(target), via_file: true, tmux: false, context: CTX_10,
                   stamp: StateFix::Unreadable, state: ST_UNREADABLE },
        TickCase { reason: "state-stale", beat_age_s: None, pane: Some(IDLE_PANE),
                   wm_seat: Some(target), via_file: true, tmux: false, context: CTX_10,
                   stamp: StateFix::Busy { age_s: STALE_S + 1 }, state: ST_STALE },
        TickCase { reason: "wm-unreadable", beat_age_s: None, pane: Some(IDLE_PANE),
                   wm_seat: None, via_file: true, tmux: false, context: CTX_10,
                   stamp: StateFix::Idle, state: ST_IDLE },
        TickCase { reason: "wm-unconsumed", beat_age_s: None, pane: Some(IDLE_PANE),
                   wm_seat: Some(target), via_file: true, tmux: false, context: CTX_10,
                   stamp: StateFix::Idle, state: ST_IDLE },
        // 席の名乗りが違う退避物と decoy は自席の根拠にしない＝4 を**通って** 5 で止まる。
        TickCase { reason: "cycle-live", beat_age_s: None, pane: Some(IDLE_PANE),
                   wm_seat: Some("other:seat"), via_file: true, tmux: false, context: CTX_10,
                   stamp: StateFix::Idle, state: ST_IDLE },
    ]
}

/// 4 条件が揃った周は注入し、**自分で打刻する**（次の周は `pointer-recent` で合図を重ねない＝
/// storm 止め・`s2-07l.109` 以降は tick-stamp だけが brake で heartbeat の mtime は見ない）。
#[test]
fn seat_tick_injects_pointer_and_stamps_on_isolated_socket() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seattick";
    let guard = start_seat(&socket, name);
    assert!(guard.ready(), "独立 socket に prompt 付きの session を立てられる");
    let state = dir.join("state");
    let wm = dir.join("wm");
    fs::create_dir_all(&wm).ok();
    stamp_idle(&state, name);
    let (wm_s, state_s) = (wm.display().to_string(), state.display().to_string());
    let args = [
        "tick", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s,
    ];

    let out = run_seat(&args);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!("seat: tick decision=inject target={name} consumed=false kind=pointer{CTX_NO_SOURCE}{ST_IDLE}{}\n", provenance(&state, "flag")),
        "`sh -i` の席は打刻しない＝消費の証拠が来ないので consumed=false（送達は成立）"
    );
    let pane = capture(&socket, name);
    assert!(
        pane.contains(&format!("seat heartbeat --target {name}")),
        "既定の 1 行が pane に現れる: {pane}"
    );
    let stamp = seat_dir_of(&state, name).join("tick-stamp");
    assert!(stamp.exists(), "自打刻が残る");

    let out = run_seat(&args);
    assert_eq!(
        stdout_of(&out),
        format!("seat: tick decision=noop reason=pointer-recent{CTX_NO_SOURCE}{ST_IDLE}{}\n", provenance(&state, "flag")),
        "自分の打刻の直後は合図を重ねない（この周も context と状態は読む）"
    );
    // 打刻を閾値の外へ倒すと、また撃つ側に戻る（「打刻が在る」ではなく経過で決まる）。
    backdate(&stamp, STALE_S + 1);
    let out = run_seat(&args);
    assert_eq!(
        stdout_of(&out),
        format!("seat: tick decision=inject target={name} consumed=false kind=pointer{CTX_NO_SOURCE}{ST_IDLE}{}\n", provenance(&state, "flag"))
    );
    // socket を消す**前**に畳む（消してからでは kill-session が届かない・実測 2026-09-10）。
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// context が cap 以上 ∧ **打刻が Busy** の席（インシデントの形）には、idle を待たずに退避の
/// pointer 1 行を注入する（`kind=externalize`・SRS FR29「idle を待たずに」・退避の合図は状態の門の
/// **外**＝planner 裁定 2026-09-11: FR29 > ADR-0015 §2.3）。payload の先頭行は退避 skill と実測値を
/// 持ち、注入後は自打刻する（自打刻は pointer の brake であって、退避の合図は次の周も送る）。
///
/// 判定は `--capture-file` の pane で通し、送信だけ独立 socket の席へ通す。busy を理由に noop
/// する実装はこの席を誰も止められない＝auto-compact に至る（bd `s2-07l.89`）。
#[test]
fn seat_tick_injects_externalize_pointer_when_context_reaches_cap_while_busy() {
    // cap = 60 の**等号側**（60 ちょうど）も撃つ: `>=` を `>` へ緩める変異はここで落ちる。
    for pct in [96_u64, 60] {
        let dir = tmp();
        let socket = socket_of(&dir);
        let name = "seatovercap";
        let guard = start_seat(&socket, name);
        assert!(guard.ready(), "独立 socket に prompt 付きの session を立てられる");
        let state = dir.join("state");
        let wm = dir.join("wm");
        fs::create_dir_all(&wm).ok();
        write_state(&seat_dir_of(&state, name), StateFix::Busy { age_s: 0 });
        let pane = dir.join("pane.txt");
        fs::write(&pane, busy_pane_at(pct)).ok();
        let (wm_s, state_s, pane_s) = (
            wm.display().to_string(),
            state.display().to_string(),
            pane.display().to_string(),
        );
        // `--pointer` は打刻の促しの上書きであって、実測値を運ぶ退避の行には掛からない。
        let args = [
            "tick", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
            "--state-dir", &state_s, "--capture-file", &pane_s, "--pointer", "custom-pointer",
        ];

        let out = run_seat(&args);
        assert_eq!(rc_of(&out), i32::from(RC_OK), "{pct}%: stderr={}", stderr_of(&out));
        assert_eq!(
            stdout_of(&out),
            format!("seat: tick decision=inject target={name} consumed=false kind=externalize context={pct}{ST_BUSY}{}\n", provenance(&state, "flag")),
            "{pct}%: 打刻が Busy でも退避の合図は送る（state の列は busy のまま載る・busy な席は queue＝consumed=false）"
        );
        let seen = capture(&socket, name);
        assert!(seen.contains("/ready-compaction"), "{pct}%: 退避 skill の名が届く: {seen}");
        assert!(seen.contains(&format!("{pct}%")) && seen.contains("60%"), "{pct}%: 実測値と cap が届く: {seen}");
        assert!(!seen.contains("seat heartbeat"), "{pct}%: heartbeat の pointer ではない: {seen}");
        assert!(!seen.contains("custom-pointer"), "{pct}%: --pointer は退避の行を上書きしない: {seen}");
        let stamp = seat_dir_of(&state, name).join("tick-stamp");
        assert!(stamp.exists(), "{pct}%: 自打刻が残る");
        let recorded = fs::read_to_string(tick_file(&state, name)).unwrap_or_default();
        assert!(
            recorded.contains(&format!(
                r#""what":"decision=inject target=seatovercap consumed=false kind=externalize context={pct}{ST_BUSY}{}""#,
                provenance(&state, "flag")
            )),
            "{pct}%: 記録にも kind と context と state と置き場の出所が載る: {recorded}"
        );

        // 退避の合図には brake を掛けない（planner 裁定 2026-09-12 案 A・`s2-07l.109`）: 自打刻の直後の
        // 周も cap 以上なら再び送る（cap を超えたままの席を次の周で拾う＝盲点は tick の周期だけ）。
        let out = run_seat(&args);
        assert_eq!(
            stdout_of(&out),
            format!("seat: tick decision=inject target={name} consumed=false kind=externalize context={pct}{ST_BUSY}{}\n", provenance(&state, "flag")),
            "{pct}%: 自打刻の直後でも退避の合図は送る（brake は打刻の合図だけ）"
        );
        // socket を消す**前**に畳む（消してからでは kill-session が届かない・実測 2026-09-10）。
        drop(guard);
        fs::remove_dir_all(&dir).ok();
    }
}

/// cap **未満**の busy な席は、退避物 0 件 ∧ cycle lock が空いていても注入しない（`noop reason=busy`・
/// tmux 未接触）。「cap 以上」の述語を常に真へ倒す変異（cargo mutants で唯一生存した形）は、
/// 測れた席すべてへ退避の pointer を送る＝ここで落ちる。
#[test]
fn seat_tick_does_not_inject_below_cap_when_nothing_else_stops_it() {
    let dir = tmp();
    let target = "seatbelowfree";
    let state = dir.join("state");
    let wm = dir.join("wm");
    fs::create_dir_all(&wm).ok();
    write_state(&seat_dir_of(&state, target), StateFix::Busy { age_s: 0 });
    fs::write(dir.join("pane.txt"), busy_pane_at(59)).ok();
    let (wm_s, state_s, pane_s, sock_s) = (
        wm.display().to_string(),
        state.display().to_string(),
        dir.join("pane.txt").display().to_string(),
        dir.join("absent-sock").display().to_string(),
    );

    let (out, touched) = run_seat_probed(
        &dir,
        &[
            "tick", "--target", target, "--wm-dir", &wm_s, "--tmux-socket", &sock_s,
            "--state-dir", &state_s, "--capture-file", &pane_s,
        ],
    );

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!("seat: tick decision=noop reason=busy context=59{ST_BUSY}{}\n", provenance(&state, "flag"))
    );
    assert!(!touched, "cap 未満の busy な席には 1 key も送らない（tmux を撃たない）");
    fs::remove_dir_all(&dir).ok();
}

/// 注入が**成立しなかった**周は打刻しない（storm 止めの極性の裏側）: 打刻を送達の前へ動かすと、
/// 退避の促しが届かないまま次の周が fresh で黙る＝cap 以上の席を握り潰す。pane は読めるが
/// tmux を撃てない席（shim）で `decision=error reason=inject-…`・rc 1・stamp 不在。
#[test]
fn seat_tick_does_not_stamp_when_externalize_injection_fails() {
    let dir = tmp();
    let target = "seatcapfail";
    let state = dir.join("state");
    let wm = dir.join("wm");
    fs::create_dir_all(&wm).ok();
    write_state(&seat_dir_of(&state, target), StateFix::Busy { age_s: 0 });
    fs::write(dir.join("pane.txt"), OVER_CAP_BUSY_PANE).ok();
    let (wm_s, state_s, pane_s, sock_s) = (
        wm.display().to_string(),
        state.display().to_string(),
        dir.join("pane.txt").display().to_string(),
        dir.join("absent-sock").display().to_string(),
    );

    let (out, touched) = run_seat_probed(
        &dir,
        &[
            "tick", "--target", target, "--wm-dir", &wm_s, "--tmux-socket", &sock_s,
            "--state-dir", &state_s, "--capture-file", &pane_s,
        ],
    );

    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "stdout={}", stdout_of(&out));
    assert!(
        stderr_of(&out).starts_with("seat: tick decision=error reason=inject-"),
        "注入の断りは inject- の前置き: {}",
        stderr_of(&out)
    );
    assert!(stderr_of(&out).contains(" context=96"), "context は評価済み: {}", stderr_of(&out));
    assert!(touched, "注入だけが tmux に当たる");
    assert!(
        !seat_dir_of(&state, target).join("tick-stamp").exists(),
        "成立しなかった注入では打刻しない（次の周も撃つ）"
    );
    fs::remove_dir_all(&dir).ok();
}

/// cap **未満** ∧ busy は従来どおり `noop reason=busy` のまま、判定行に context の値が載る。
#[test]
fn seat_tick_reports_context_when_busy_below_cap() {
    let target = "seatbelowcap";
    // 59 は cap の**直下**（`>=` を `>` へ緩めても 60 で落ちる歯と対で、境界を両側から撃つ）。
    for (pane, context) in [(BELOW_CAP_BUSY_PANE.to_owned(), " context=12"), (busy_pane_at(59), " context=59")] {
        let case = TickCase { reason: "busy", beat_age_s: None, pane: None,
                              wm_seat: Some(target), via_file: true, tmux: false, context,
                              stamp: StateFix::Busy { age_s: 0 }, state: ST_BUSY };
        let dir = tmp();
        let state = prepare_tick_case(&dir, &case, target);
        fs::write(dir.join("pane.txt"), &pane).ok();
        let (out, touched) = run_tick_case(&dir, &case, target, &state);
        assert_tick_case(&out, touched, &case, 0, &state);
        fs::remove_dir_all(&dir).ok();
    }
}

/// statusline が無い ∧ idle は従来の判定（退避物）へ進み、判定行に `context=unmeasured` と
/// 理由が載る（測れないことを理由に注入も停止もしない・AC9 条 3 と同じ極性）。
#[test]
fn seat_tick_proceeds_with_unmeasured_context_without_statusline() {
    let target = "seatnostatus";
    let cases = [
        (NO_STATUSLINE_IDLE_PANE, " context=unmeasured reason=pane-no-statusline"),
        // 候補は在るが健全性を外れた周も同じ極性（捏造値で cap を超えない・注入しない）。
        (OUT_OF_BOUND_IDLE_PANE, " context=unmeasured reason=pane-out-of-bound"),
    ];
    for (at, (pane, context)) in cases.into_iter().enumerate() {
        let case = TickCase { reason: "wm-unconsumed", beat_age_s: None, pane: Some(pane),
                              wm_seat: Some(target), via_file: true, tmux: false, context,
                              stamp: StateFix::Idle, state: ST_IDLE };
        let dir = tmp();
        let state = prepare_tick_case(&dir, &case, target);
        let (out, touched) = run_tick_case(&dir, &case, target, &state);
        assert_tick_case(&out, touched, &case, at, &state);
        fs::remove_dir_all(&dir).ok();
    }
}

/// heartbeat が fresh の周も cap 以上の pane と打刻を読む＝context も state も判定行に載る
/// （`s2-07l.109`・鮮度 gate 撤去。鮮度で止まる実装は `heartbeat-fresh` で何も載せない＝RED）。
/// lock が live なので退避の合図は送らず、状態の門（Busy）で止まる。
#[test]
fn seat_tick_without_freshness_gate_reads_context_and_state_when_fresh() {
    let target = "seatfreshcap";
    let case = TickCase { reason: "busy", beat_age_s: Some(STALE_S - 1),
                          pane: Some(OVER_CAP_BUSY_PANE), wm_seat: Some("other:seat"),
                          via_file: true, tmux: false, context: " context=96",
                          stamp: StateFix::Busy { age_s: 0 }, state: ST_BUSY };
    let dir = tmp();
    let state = prepare_tick_case(&dir, &case, target);
    let (out, touched) = run_tick_case(&dir, &case, target, &state);
    assert_tick_case(&out, touched, &case, 0, &state);
    fs::remove_dir_all(&dir).ok();
}

/// cap 以上でも **自席の未 consumed 退避物が在る**（退避済み）／退避物の dir を読めない／
/// cycle lock が live の周は pointer を注入せず次の条件へ進む（planner 裁定 2026-09-11・livelock の補正）: 退避済みの席は
/// `/clear` 前で cap 以上のままなので、注入すると毎周 pointer を重ねて cycle に一度も落ちない。
/// busy なら `busy`、idle なら `wm-unconsumed` / `wm-unreadable`（lock は TTL 内＝cycle は評価しない）。
#[test]
fn seat_tick_falls_through_to_wm_when_parked_over_cap() {
    let target = "seatparkedcap";
    let cases = [
        TickCase { reason: "busy", beat_age_s: None, pane: Some(OVER_CAP_BUSY_PANE),
                   wm_seat: Some(target), via_file: true, tmux: false, context: " context=96",
                   stamp: StateFix::Busy { age_s: 0 }, state: ST_BUSY },
        TickCase { reason: "wm-unconsumed", beat_age_s: None, pane: Some(OVER_CAP_IDLE_PANE),
                   wm_seat: Some(target), via_file: true, tmux: false, context: " context=96",
                   stamp: StateFix::Idle, state: ST_IDLE },
        TickCase { reason: "wm-unreadable", beat_age_s: None, pane: Some(OVER_CAP_IDLE_PANE),
                   wm_seat: None, via_file: true, tmux: false, context: " context=96",
                   stamp: StateFix::Idle, state: ST_IDLE },
        // 退避物 0 件でも他の cycle が走っている（lock が TTL 内）周は注入しない（排他）。
        TickCase { reason: "cycle-live", beat_age_s: None, pane: Some(OVER_CAP_IDLE_PANE),
                   wm_seat: Some("other:seat"), via_file: true, tmux: false, context: " context=96",
                   stamp: StateFix::Idle, state: ST_IDLE },
    ];
    for (at, case) in cases.iter().enumerate() {
        let dir = tmp();
        let state = prepare_tick_case(&dir, case, target);
        let (out, touched) = run_tick_case(&dir, case, target, &state);
        assert_tick_case(&out, touched, case, at, &state);
        fs::remove_dir_all(&dir).ok();
    }
}

/// 実行系が回らない周は `decision=error` と **rc 1**（noop の語彙を汚さない）。
#[test]
fn seat_tick_reports_error_with_rc_one_when_state_dir_is_unresolvable() {
    let dir = tmp();
    let wm = dir.join("wm");
    fs::create_dir_all(&wm).ok();

    // `--state-dir` を渡さない周は repo の git 設定から解く。repo の外では解けない。
    let out = run_seat_in(&dir, &["tick", "--target", "seaterr", "--wm-dir", &wm.display().to_string()]);

    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "失敗は rc 1（timer から見て成功に見せない）");
    assert_eq!(stdout_of(&out), "", "失敗した周は stdout 0 行");
    assert_eq!(stderr_of(&out), "seat: tick decision=error reason=state-dir\n");
    fs::remove_dir_all(&dir).ok();
}

// flip-check: retroactive s2-07l.50
// 下の 3 本は後から足す歯である。実装（`src/seat/*.rs`）は 1 行も変えておらず、穴は歯の
// 側に在った——base に対して新しく赤くなる歯を作れないので、逃がしを札 1 行で明示する。

/// pane は読めるが tmux を撃てない周は `decision=error reason=inject-…` と **rc 1**。
///
/// 既存の error 歯は `run()` の早期 return（`--state-dir` が解けない組）しか撃たず、
/// **judgment を経由した Error 腕**——順序 4 を通って `inject_pointer` が注入を断られる
/// 組——に届いていなかった。pane を `--capture-file` で読ませると tmux を 1 度も撃たずに
/// 順序 2 を通れるので、**注入だけが tmux に当たって落ちる**組が作れる。
///
/// reason の続き（`tmux-failed` 等）は固定しない: 注入の断りは字面が noop の語彙と重なる
/// ので、器が約束しているのは **`inject-` の前置きで分けること**だけである。
#[test]
fn seat_tick_reports_error_when_pane_is_readable_but_tmux_is_unreachable() {
    let dir = tmp();
    let target = "seatunreach";
    let state = dir.join("state");
    let wm = dir.join("wm");
    // 順序 3 は**自席の**退避物だけを見る。別席の名乗りと decoy は「3 を通った」側の材料で、
    // ここで止まると順序 4 へ届かず、この歯は Error 腕を 1 度も撃たない。
    wm_file(&wm, "working-memory.parked.md", "other:seat");
    wm_decoys(&wm, target);
    stamp_idle(&state, target);
    fs::write(dir.join("pane.txt"), IDLE_PANE).expect("pane fixture を置ける");
    let (wm_s, state_s) = (wm.display().to_string(), state.display().to_string());
    let (sock_s, pane_s) = (
        dir.join("absent-sock").display().to_string(),
        dir.join("pane.txt").display().to_string(),
    );

    let out = run_seat(&[
        "tick", "--target", target, "--wm-dir", &wm_s, "--tmux-socket", &sock_s,
        "--state-dir", &state_s, "--capture-file", &pane_s,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "注入を断られた周は rc 1");
    assert!(
        !seat_dir_of(&state, target).join("tick-stamp").exists(),
        "成立しなかった注入では打刻しない（打刻を送達の前へ動かす変異はここで落ちる）"
    );
    assert_eq!(stdout_of(&out), "", "失敗した周は stdout 0 行");
    assert!(
        stderr_of(&out).starts_with("seat: tick decision=error reason=inject-"),
        "注入の断りは inject- の前置きで noop の語彙と分ける: {}",
        stderr_of(&out)
    );
    let recorded = fs::read_to_string(tick_file(&state, target)).unwrap_or_default();
    assert!(
        recorded
            .lines()
            .last()
            .is_some_and(|line| line.contains(r#""what":"decision=error reason=inject-"#)),
        "記録の末尾 1 行も理由まで残す（表示と記録で同じ字面）: {recorded}"
    );
    fs::remove_dir_all(&dir).ok();
}

/// `working-memory.` で始まり `.md` で終わるが **17 文字**の名前は退避物に数えない。
///
/// 長さの条件（前置き + 接尾の最短形 = 18 文字）を外す変異が生き延びていた。decoy が
/// 短い名前だった間は、前置きを外す変異が**長さで**落ち、長さを外す変異は誰にも撃たれ
/// なかった——ここは長さだけが効く負例を単独で置く。
///
/// 期待は「自席の退避物なし」で順序 3 を**通って** 4 で止まる形（`cycle-live`）である。
/// 長さの条件が消えると同じ fixture が `wm-unconsumed` へ倒れる＝理由の字面が変異を捕まえる。
#[test]
fn seat_tick_ignores_short_wm_like_names() {
    let dir = tmp();
    let target = "seatshort";
    let state = dir.join("state");
    let seat = seat_dir_of(&state, target);
    fs::create_dir_all(&seat).expect("seat dir を作れる");
    // 順序 4 で止める（TTL 内の lock）＝3 を通ったことが理由の字面で分かる。
    // ★`lock_is_live` は **mtime だけ**を見て中身を読まない: `deadline:0` は失効に見えるが、
    // いま書いた file なので live 側である（既存の歯と同じ idiom）。
    fs::write(seat.join("cycle.lock"), "{\"pid\":1,\"deadline\":0}\n").expect("lock を置ける");
    write_state(&seat, StateFix::Idle);
    let wm = dir.join("wm");
    fs::create_dir_all(&wm).expect("wm dir を作れる");
    // 前置きと接尾は満たすが 17 文字＝最短形に 1 文字足りない（名乗りは自席にしておく）。
    let short = wm.join("working-memory.md");
    fs::write(&short, format!("---\nseat: {target}\n---\n\n## 計画弧\n- 続き\n"))
        .expect("短い名前の file を置ける");
    assert_eq!(
        short.file_name().and_then(std::ffi::OsStr::to_str).map(str::len),
        Some(17),
        "負例は 17 文字ちょうど（境界の 1 文字下）"
    );
    fs::write(dir.join("pane.txt"), IDLE_PANE).expect("pane fixture を置ける");
    let (wm_s, state_s) = (wm.display().to_string(), state.display().to_string());
    let (sock_s, pane_s) = (
        dir.join("absent-sock").display().to_string(),
        dir.join("pane.txt").display().to_string(),
    );

    let out = run_seat(&[
        "tick", "--target", target, "--wm-dir", &wm_s, "--tmux-socket", &sock_s,
        "--state-dir", &state_s, "--capture-file", &pane_s,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!("seat: tick decision=noop reason=cycle-live{CTX_10}{ST_IDLE}{}\n", provenance(&state, "flag")),
        "短すぎる名前は自席の退避物に数えない＝4 を通って 5 で止まる"
    );
    // 表示の完全一致だけでは 1 段しか測れない（`wm-unconsumed` を除く assert は上の
    // 完全一致に包含されて**発火しない**）。記録側の末尾 1 行でもう 1 段測る。
    let recorded = fs::read_to_string(tick_file(&state, target)).unwrap_or_default();
    assert!(
        recorded
            .lines()
            .last()
            .is_some_and(|line| line.contains(r#""what":"decision=noop reason=cycle-live "#)),
        "記録の末尾 1 行も cycle-live（長さの条件が消えると wm-unconsumed へ倒れる）: {recorded}"
    );
    fs::remove_dir_all(&dir).ok();
}

/// 退避して止まっている席は、tick がその場で cycle を回す（裁定 (b)）。
#[test]
fn seat_tick_runs_cycle_when_parked() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatparked";
    let log = dir.join("seat.log");
    let state = dir.join("state");
    let guard = start_clearing_seat(
        &socket, name, &log, &state_file(&seat_dir_of(&state, name)), (FakeStamp::Now, FakeStamp::Now),
    );
    assert!(guard.ready(), "fake な席を立てられる");
    let wm = dir.join("wm");
    wm_file(&wm, "working-memory.parked.md", name);
    stamp_idle(&state, name);
    let (wm_s, state_s) = (wm.display().to_string(), state.display().to_string());

    let out = run_seat(&[
        "tick", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!("seat: tick decision=noop reason=wm-unconsumed{CTX_NO_SOURCE} cycle=done{ST_IDLE} cycle-stamp=none{}\n", provenance(&state, "flag")),
        "判定は noop のまま・context の後ろに cycle を回したことを足し、state の列が続く（置き場の出所は最後）"
    );
    assert_eq!(
        fs::read_to_string(&log).unwrap_or_default(),
        "/clear\n/rebrief\n",
        "作り直して復元した"
    );
    // socket を消す**前**に畳む（消してからでは kill-session が届かない・実測 2026-09-10）。
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// 退避を終えた席は heartbeat を**直前**に打っていることが多い。それでも退避物が在る周は cycle を
/// 評価する（退避物の存在 = 席の「作り直してよい」の申告・`s2-07l.105`・user 直命 2026-09-11
/// 「流石に長すぎだろ」）。`.105` は鮮度 gate を飛ばす特例で、`.109` で鮮度 gate ごと無くなった
/// ＝heartbeat の mtime は判定入力ではない（この歯は heartbeat が今でも結果が変わらないことの pin）。
#[test]
fn seat_tick_cycles_freshly_stamped_seat_when_own_wm_is_unconsumed() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatfreshparked";
    let log = dir.join("seat.log");
    let state = dir.join("state");
    let guard = start_clearing_seat(
        &socket, name, &log, &state_file(&seat_dir_of(&state, name)), (FakeStamp::Now, FakeStamp::Now),
    );
    assert!(guard.ready(), "fake な席を立てられる");
    let seat = seat_dir_of(&state, name);
    fs::create_dir_all(&seat).expect("seat dir を作れる");
    // 直前の打刻（mtime = いま）＝鮮度だけなら fresh で止まる周。
    fs::write(seat.join("heartbeat"), "").expect("打刻を置ける");
    let wm = dir.join("wm");
    wm_file(&wm, "working-memory.parked.md", name);
    stamp_idle(&state, name);
    let (wm_s, state_s) = (wm.display().to_string(), state.display().to_string());

    let out = run_seat(&[
        "tick", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!("seat: tick decision=noop reason=wm-unconsumed{CTX_NO_SOURCE} cycle=done{ST_IDLE} cycle-stamp=none{}\n", provenance(&state, "flag")),
        "打刻が fresh でも退避物が在れば鮮度を飛ばして cycle を評価する（`cycle=` が付く）"
    );
    assert_eq!(
        fs::read_to_string(&log).unwrap_or_default(),
        "/clear\n/rebrief\n",
        "作り直して復元した"
    );
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// cycle を評価した周は `cycle-stamp` を打ち、次の周以降は同じ退避物が残っていても
/// `seat.tick_stale_s` 未満なら cycle を評価しない（`cycle-recent`・`/clear` を送らない＝
/// 偽の席の受信 log が増えない）。`/clear` は不可逆の口（N1）で、復元されない退避物へ
/// 5 分ごとに繰り返してはならない（`s2-07l.110`・裁定 (a)）。
///
/// base は cycle を回した事実を残さず、2 度目の周も `cycle=done` になる（RED）。
#[test]
fn seat_tick_backs_off_after_a_recent_cycle() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatbackoff";
    let log = dir.join("seat.log");
    let state = dir.join("state");
    let guard = start_clearing_seat(
        &socket, name, &log, &state_file(&seat_dir_of(&state, name)), (FakeStamp::Now, FakeStamp::Now),
    );
    assert!(guard.ready(), "fake な席を立てられる");
    let wm = dir.join("wm");
    wm_file(&wm, "working-memory.parked.md", name);
    stamp_idle(&state, name);
    let (wm_s, state_s) = (wm.display().to_string(), state.display().to_string());
    let args = [
        "tick", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s,
    ];

    let first = run_seat(&args);
    assert_eq!(rc_of(&first), i32::from(RC_OK), "stderr={}", stderr_of(&first));
    assert_eq!(
        stdout_of(&first),
        format!("seat: tick decision=noop reason=wm-unconsumed{CTX_NO_SOURCE} cycle=done{ST_IDLE} cycle-stamp=none{}\n", provenance(&state, "flag")),
        "1 周目: 打刻が無いので cycle を回す（stamp は none・列は cycle → state → cycle-stamp → 出所）"
    );
    let stamp = seat_dir_of(&state, name).join("cycle-stamp");
    assert!(stamp.is_file(), "cycle を評価した周は cycle-stamp を打つ");
    let stamped_at = mtime_of(&stamp);

    // 退避物は残ったまま（偽の席は /rebrief で consume しない）。
    let second = run_seat(&args);
    assert_eq!(rc_of(&second), i32::from(RC_OK), "stderr={}", stderr_of(&second));
    let line = stdout_of(&second);
    assert_eq!(
        tick_token(&line, "reason").as_deref(),
        Some("cycle-recent"),
        "2 周目: 直前に cycle を評価したので評価しない: {line}"
    );
    assert!(tick_token(&line, "cycle").is_none(), "2 周目は cycle を評価しない（`cycle=` が付かない）: {line}");
    let age = tick_token(&line, "cycle-stamp")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(u64::MAX);
    assert!(age < STALE_S, "cycle-stamp の age（秒）は閾値未満: {line}");
    assert!(
        line.ends_with(&format!("{ST_IDLE} cycle-stamp={age}{}\n", provenance(&state, "flag"))),
        "state の列 → cycle-stamp → 置き場の出所の順のまま: {line}"
    );
    assert_eq!(
        fs::read_to_string(&log).unwrap_or_default(),
        "/clear\n/rebrief\n",
        "2 周目は /clear を送らない（偽の席の受信は 1 周目の 2 行のまま）"
    );
    assert_eq!(
        mtime_of(&stamp),
        stamped_at,
        "見送った周は stamp を打ち直さない（打ち直すと back-off が永久になる）"
    );
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// back-off は永久ではない: cycle-stamp が `seat.tick_stale_s` 以上前なら再び cycle を
/// 評価する（復元が失敗したまま放置された席を、次の stale な周で拾い直す）。
#[test]
fn seat_tick_re_evaluates_cycle_once_the_cycle_stamp_is_stale() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatstalestamp";
    let log = dir.join("seat.log");
    let state = dir.join("state");
    let guard = start_clearing_seat(
        &socket, name, &log, &state_file(&seat_dir_of(&state, name)), (FakeStamp::Now, FakeStamp::Now),
    );
    assert!(guard.ready(), "fake な席を立てられる");
    stamp_idle(&state, name);
    let stamp = seat_dir_of(&state, name).join("cycle-stamp");
    fs::write(&stamp, "0\n").expect("stamp を置ける");
    // 境界は**未満**: 経過が閾値ちょうどの周は評価する（`<=` にすると見送る）。
    backdate(&stamp, STALE_S);
    let wm = dir.join("wm");
    wm_file(&wm, "working-memory.parked.md", name);
    let (wm_s, state_s) = (wm.display().to_string(), state.display().to_string());

    let out = run_seat(&[
        "tick", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    let line = stdout_of(&out);
    assert_eq!(tick_token(&line, "reason").as_deref(), Some("wm-unconsumed"), "{line}");
    assert_eq!(tick_token(&line, "cycle").as_deref(), Some("done"), "stale な stamp は cycle を止めない: {line}");
    let age = tick_token(&line, "cycle-stamp")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    assert!(age >= STALE_S, "判定行に stamp の age（秒）を載せる: {line}");
    assert_eq!(fs::read_to_string(&log).unwrap_or_default(), "/clear\n/rebrief\n", "作り直して復元した");
    assert!(
        mtime_of(&stamp).elapsed().is_ok_and(|since| since.as_secs() < STALE_S),
        "評価した周は stamp を打ち直す"
    );
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// stamp を読めない周は「無い」に読み替えず、cycle を評価しない（`cycle-stamp-unreadable`・
/// 読めないことを理由に不可逆の `/clear` へ倒さない＝N1 の向き・planner 裁定 2026-09-11）。
/// stamp を自分を指す symlink（loop）にして metadata を読めなくする（state の打刻は Idle のまま
/// ＝状態の門は通る）。tmux は叩かない。
#[test]
fn seat_tick_does_not_evaluate_cycle_when_the_cycle_stamp_is_unreadable() {
    let dir = tmp();
    let target = "seatbadstamp";
    let state = dir.join("state");
    stamp_idle(&state, target);
    let stamp = seat_dir_of(&state, target).join("cycle-stamp");
    std::os::unix::fs::symlink("cycle-stamp", &stamp).expect("自分を指す symlink を置ける");
    assert!(fs::metadata(&stamp).is_err(), "負例の前提: stamp の metadata は読めない（loop）");
    let wm = dir.join("wm");
    wm_file(&wm, "working-memory.parked.md", target);
    fs::write(dir.join("pane.txt"), IDLE_PANE).expect("pane fixture を置ける");
    let (wm_s, state_s) = (wm.display().to_string(), state.display().to_string());
    let (sock_s, pane_s) = (
        dir.join("absent-sock").display().to_string(),
        dir.join("pane.txt").display().to_string(),
    );

    let (out, touched) = run_seat_probed(&dir, &[
        "tick", "--target", target, "--wm-dir", &wm_s, "--tmux-socket", &sock_s,
        "--state-dir", &state_s, "--capture-file", &pane_s,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!("seat: tick decision=noop reason=cycle-stamp-unreadable{CTX_10}{ST_IDLE} cycle-stamp=unreadable{}\n", provenance(&state, "flag")),
        "読めない stamp は評価しない側へ倒す（`cycle=` が付かない）"
    );
    assert!(!touched, "1 key も送らない");
    fs::remove_dir_all(&dir).ok();
}

/// (a) heartbeat が**今**（fresh）でも、打刻 Idle・context が cap 未満・退避物なし・lock なしなら
/// 打刻の合図を注入する（`s2-07l.109`・鮮度 gate 撤去）。鮮度で止まる実装は `heartbeat-fresh` の
/// noop になる（RED）。heartbeat の mtime は判定入力ではない。
#[test]
fn seat_tick_without_freshness_gate_injects_pointer_when_heartbeat_is_fresh() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatfreshinject";
    let guard = start_seat(&socket, name);
    assert!(guard.ready(), "独立 socket に prompt 付きの session を立てられる");
    let state = dir.join("state");
    let wm = dir.join("wm");
    fs::create_dir_all(&wm).ok();
    stamp_idle(&state, name);
    fs::write(seat_dir_of(&state, name).join("heartbeat"), "").expect("直前の打刻を置ける");
    let (wm_s, state_s) = (wm.display().to_string(), state.display().to_string());

    let out = run_seat(&[
        "tick", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!("seat: tick decision=inject target={name} consumed=false kind=pointer{CTX_NO_SOURCE}{ST_IDLE}{}\n", provenance(&state, "flag")),
        "heartbeat が今でも注入する（鮮度は判定入力ではない）"
    );
    assert!(capture(&socket, name).contains(&format!("seat heartbeat --target {name}")), "合図が届く");
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// (b) heartbeat と tick-stamp が**今**でも、context が cap 以上なら退避の合図を送る＝「打刻の
/// 直後に cap を超えた席が最大 `seat.tick_stale_s` 見えない」盲点の消滅を pin（`s2-07l.109`）。
/// 退避の合図は `pointer-recent` の周でも送る（planner 裁定 2026-09-12・案 A）。
#[test]
fn seat_tick_without_freshness_gate_sends_externalize_when_over_cap_while_fresh() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatfreshovercap";
    let guard = start_seat(&socket, name);
    assert!(guard.ready(), "独立 socket に prompt 付きの session を立てられる");
    let state = dir.join("state");
    let wm = dir.join("wm");
    fs::create_dir_all(&wm).ok();
    let seat = seat_dir_of(&state, name);
    write_state(&seat, StateFix::Idle);
    fs::write(seat.join("heartbeat"), "").expect("直前の打刻を置ける");
    fs::write(seat.join("tick-stamp"), "").expect("直前の自打刻を置ける");
    let pane = dir.join("pane.txt");
    fs::write(&pane, busy_pane_at(96)).ok();
    let (wm_s, state_s, pane_s) = (
        wm.display().to_string(),
        state.display().to_string(),
        pane.display().to_string(),
    );

    let out = run_seat(&[
        "tick", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s, "--capture-file", &pane_s,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!("seat: tick decision=inject target={name} consumed=false kind=externalize context=96{ST_IDLE}{}\n", provenance(&state, "flag")),
        "打刻の直後でも cap 以上なら退避の合図（盲点の消滅）"
    );
    assert!(capture(&socket, name).contains("/ready-compaction"), "退避 skill の名が届く");
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// (c) heartbeat が今でも、打刻が Busy なら注入しない（鮮度を外しても fail-closed は不変）。
#[test]
fn seat_tick_without_freshness_gate_keeps_busy_closed() {
    let target = "seatfreshbusyclosed";
    let case = TickCase { reason: "busy", beat_age_s: Some(0), pane: Some(IDLE_PANE),
                          wm_seat: Some("other:seat"), via_file: true, tmux: false, context: CTX_10,
                          stamp: StateFix::Busy { age_s: 0 }, state: ST_BUSY };
    let dir = tmp();
    let state = prepare_tick_case(&dir, &case, target);
    let (out, touched) = run_tick_case(&dir, &case, target, &state);
    assert_tick_case(&out, touched, &case, 0, &state);
    fs::remove_dir_all(&dir).ok();
}

/// (d) heartbeat が今でも、打刻が missing / unreadable / stale ならそれぞれの理由で止まる＝鮮度に
/// 隠れていた理由が判定行に出る（missing を idle に・stale を busy に読み替えない）。
#[test]
fn seat_tick_without_freshness_gate_surfaces_state_reasons() {
    let target = "seatfreshstate";
    let cases = [
        TickCase { reason: "state-missing", beat_age_s: Some(0), pane: Some(IDLE_PANE),
                   wm_seat: Some("other:seat"), via_file: true, tmux: false, context: CTX_10,
                   stamp: StateFix::Absent, state: ST_MISSING },
        TickCase { reason: "state-unreadable", beat_age_s: Some(0), pane: Some(IDLE_PANE),
                   wm_seat: Some("other:seat"), via_file: true, tmux: false, context: CTX_10,
                   stamp: StateFix::Unreadable, state: ST_UNREADABLE },
        TickCase { reason: "state-stale", beat_age_s: Some(0), pane: Some(IDLE_PANE),
                   wm_seat: Some("other:seat"), via_file: true, tmux: false, context: CTX_10,
                   stamp: StateFix::Busy { age_s: STALE_S + 1 }, state: ST_STALE },
    ];
    for (at, case) in cases.iter().enumerate() {
        let dir = tmp();
        let state = prepare_tick_case(&dir, case, target);
        let (out, touched) = run_tick_case(&dir, case, target, &state);
        assert_tick_case(&out, touched, case, at, &state);
        fs::remove_dir_all(&dir).ok();
    }
}

/// (e) 打刻の合図の頻度は **tick 自身の打刻（tick-stamp）**で決める（planner 裁定 2026-09-12・案 A）:
/// 注入した直後の周は `pointer-recent`（pointer を送らない・context と状態はこの周も読む）、
/// tick-stamp が閾値**ちょうど以上**なら再び注入（境界「未満」を pin）、不在なら注入。
/// heartbeat の mtime は見ない（(a) が持つ）。
#[test]
fn seat_tick_without_freshness_gate_backs_off_pointer_by_tick_stamp() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatpointerrecent";
    let guard = start_seat(&socket, name);
    assert!(guard.ready(), "独立 socket に prompt 付きの session を立てられる");
    let state = dir.join("state");
    let wm = dir.join("wm");
    fs::create_dir_all(&wm).ok();
    stamp_idle(&state, name);
    let (wm_s, state_s) = (wm.display().to_string(), state.display().to_string());
    let args = [
        "tick", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s,
    ];
    let injected = format!("seat: tick decision=inject target={name} consumed=false kind=pointer{CTX_NO_SOURCE}{ST_IDLE}{}\n", provenance(&state, "flag"));

    let first = run_seat(&args);
    assert_eq!(stdout_of(&first), injected, "tick-stamp 不在 → 注入: stderr={}", stderr_of(&first));
    let stamp = seat_dir_of(&state, name).join("tick-stamp");
    assert!(stamp.is_file(), "自打刻が残る");

    let second = run_seat(&args);
    assert_eq!(rc_of(&second), i32::from(RC_OK), "stderr={}", stderr_of(&second));
    assert_eq!(
        stdout_of(&second),
        format!("seat: tick decision=noop reason=pointer-recent{CTX_NO_SOURCE}{ST_IDLE}{}\n", provenance(&state, "flag")),
        "直後の周は合図を重ねない（context と状態はこの周も読んで載せる）"
    );
    let heard = capture(&socket, name).matches("seat heartbeat --target").count();
    assert_eq!(heard, 1, "pane に届いた合図は 1 周目の 1 本だけ");

    // 境界は**未満**: 経過が閾値ちょうどの周は注入する（`<=` にすると見送る）。
    backdate(&stamp, STALE_S);
    let third = run_seat(&args);
    assert_eq!(stdout_of(&third), injected, "閾値ちょうど以上 → 再び注入: stderr={}", stderr_of(&third));
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// 打刻の合図の brake（`pointer-recent`）は合図にだけ効く: tick-stamp が今でも、自席の退避物が在る
/// idle の席は cycle を評価する（`s2-07l.109`・lens-109 F3。brake を退避物の判定より前に置く変異は
/// `.105` の飢餓〔退避物が在るのに cycle されない〕を静かに戻す）。
#[test]
fn seat_tick_without_freshness_gate_cycles_parked_seat_even_when_pointer_recent() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatparkedrecent";
    let log = dir.join("seat.log");
    let state = dir.join("state");
    let guard = start_clearing_seat(
        &socket, name, &log, &state_file(&seat_dir_of(&state, name)), (FakeStamp::Now, FakeStamp::Now),
    );
    assert!(guard.ready(), "fake な席を立てられる");
    stamp_idle(&state, name);
    fs::write(seat_dir_of(&state, name).join("tick-stamp"), "").expect("直前の自打刻を置ける");
    let wm = dir.join("wm");
    wm_file(&wm, "working-memory.parked.md", name);
    let (wm_s, state_s) = (wm.display().to_string(), state.display().to_string());

    let out = run_seat(&[
        "tick", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!("seat: tick decision=noop reason=wm-unconsumed{CTX_NO_SOURCE} cycle=done{ST_IDLE} cycle-stamp=none{}\n", provenance(&state, "flag")),
        "brake の周でも退避物が在れば cycle を評価する（`pointer-recent` にならない）"
    );
    assert_eq!(fs::read_to_string(&log).unwrap_or_default(), "/clear\n/rebrief\n", "作り直して復元した");
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// heartbeat が今で退避物が在っても、打刻が Busy の席には送らない（cycle も評価しない＝`cycle=` が
/// 付かない）。状態の門は heartbeat の mtime に依らない。
#[test]
fn seat_tick_freshly_stamped_seat_with_own_wm_is_still_not_cycled_when_busy() {
    let target = "seatfreshbusy";
    let case = TickCase { reason: "busy", beat_age_s: Some(STALE_S - 1), pane: Some(IDLE_PANE),
                          wm_seat: Some(target), via_file: true, tmux: false, context: CTX_10,
                          stamp: StateFix::Busy { age_s: 0 }, state: ST_BUSY };
    let dir = tmp();
    let state = prepare_tick_case(&dir, &case, target);
    let (out, touched) = run_tick_case(&dir, &case, target, &state);
    assert_tick_case(&out, touched, &case, 0, &state);
    fs::remove_dir_all(&dir).ok();
}

/// 自席の退避物が無い周も heartbeat が fresh なら鮮度で止まらず、pane を読んで後段の条件へ
/// 進む（`s2-07l.109`・`.105` の「退避物が在る周だけ飛ばす」特例は鮮度ごと消えた）: 他席の
/// 名乗りだけの周は lock（TTL 内）で止まり、退避物の dir を読めない周は `wm-unreadable`。
/// 他席の文脈で cycle しない・読めない周を「在る」に読み替えない、は不変。
#[test]
fn seat_tick_without_freshness_gate_reads_pane_without_own_wm() {
    let target = "seatfreshother";
    let cases = [
        TickCase { reason: "cycle-live", beat_age_s: Some(STALE_S - 1), pane: Some(IDLE_PANE),
                   wm_seat: Some("other:seat"), via_file: true, tmux: false, context: CTX_10,
                   stamp: StateFix::Idle, state: ST_IDLE },
        TickCase { reason: "wm-unreadable", beat_age_s: Some(STALE_S - 1), pane: Some(IDLE_PANE),
                   wm_seat: None, via_file: true, tmux: false, context: CTX_10,
                   stamp: StateFix::Idle, state: ST_IDLE },
    ];
    for (at, case) in cases.iter().enumerate() {
        let dir = tmp();
        let state = prepare_tick_case(&dir, case, target);
        let (out, touched) = run_tick_case(&dir, case, target, &state);
        assert_tick_case(&out, touched, case, at, &state);
        fs::remove_dir_all(&dir).ok();
    }
}

/// 打刻 file が**無い**席（hook が載っていない）・**読めない**席へは送達しても消費を測れない＝
/// `consumed=unknown` に **`reason=`** を添える（missing を消費と読み替えない・`false` とも混ぜない・
/// 憲法 C10 の測定 / 未測定の弁別・`s2-07l.112`）。送達（目印が現れた）は成立ゆえ rc 0・記録は残る。
#[test]
fn seat_evidence_inject_reports_unknown_with_reason_when_stamp_file_is_missing_or_unreadable() {
    for (fix, reason) in [(StateFix::Absent, "state-missing"), (StateFix::Unreadable, "state-unreadable")] {
        let dir = tmp();
        let socket = socket_of(&dir);
        let name = "seat-nostamp";
        let guard = start_seat(&socket, name);
        assert!(guard.ready(), "{reason}: 独立 socket に prompt 付きの session を立てられる");
        let state = dir.join("state");
        write_state(&seat_dir_of(&state, name), fix);
        let payload = ": seat-e2e-nostamp";

        let out = run_seat(&[
            "inject", "--target", name, "--tmux-socket", &socket,
            "--state-dir", &state.display().to_string(), "--text", payload,
        ]);

        assert_eq!(rc_of(&out), i32::from(RC_OK), "{reason}: stderr={}", stderr_of(&out));
        assert_eq!(
            stdout_of(&out),
            format!(
                "seat: inject delivered target={name} bytes={} consumed=unknown reason={reason}{}\n",
                payload.len(),
                provenance(&state, "flag")
            ),
            "{reason}: 測れない周は unknown に理由を添える（true / false と混ぜない）"
        );
        assert!(capture(&socket, name).contains("seat-e2e-nostamp"), "{reason}: 字面は現れている（送達は成立）");
        assert!(tick_file(&state, name).exists(), "{reason}: 送達した周は記録する");
        drop(guard);
        fs::remove_dir_all(&dir).ok();
    }
}

// ─────────────────── 作り直しと送達の証拠（打刻由来・`s2-07l.112`・接頭辞 `seat_evidence_`） ───────────────────

/// 作り直しの証拠は **`/clear` の送達 ts 以後に足された `SessionStart` の打刻**だけ（設計 seat-state.md §6）。
/// pane は `--capture-file` で [`IDLE_PANE`] に固定し `/clear` の echo を**一度も**見せない＝echo を正の
/// 証拠に採る実装（base）は 30 s 待って `clear-unconfirmed` になる（RED）。復元の消費も打刻で確認する。
#[test]
fn seat_evidence_cycle_confirms_rebuild_by_session_start_stamp_without_echo() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatevclear";
    let log = dir.join("seat.log");
    let state = dir.join("state");
    let stamps = state_file(&seat_dir_of(&state, name));
    let guard = start_clearing_seat(&socket, name, &log, &stamps, (FakeStamp::Now, FakeStamp::Now));
    assert!(guard.ready(), "打刻する偽の席を立てられる");
    let wm = dir.join("wm");
    wm_file(&wm, "working-memory.ev.md", name);
    stamp_idle(&state, name);
    let before = fs::read_to_string(&stamps).unwrap_or_default().lines().count();
    let pane = dir.join("pane.txt");
    fs::write(&pane, IDLE_PANE).ok();
    let (wm_s, state_s, pane_s) = (
        wm.display().to_string(),
        state.display().to_string(),
        pane.display().to_string(),
    );

    let out = run_seat(&[
        "cycle", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s, "--capture-file", &pane_s,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), format!("seat: cycle done target={name}{}\n", provenance(&state, "flag")));
    assert_eq!(fs::read_to_string(&log).unwrap_or_default(), "/clear\n/rebrief\n", "作り直しを打刻で確認して復元を送る");
    let added: Vec<String> = fs::read_to_string(&stamps)
        .unwrap_or_default()
        .lines()
        .skip(before)
        .map(str::to_owned)
        .collect();
    assert!(added.iter().any(|line| line.contains(r#""event":"SessionStart""#)), "作り直しの証拠が足されている: {added:?}");
    assert!(added.iter().any(|line| line.contains(r#""event":"UserPromptSubmit""#)), "復元の消費の証拠が足されている: {added:?}");
    // socket を消す**前**に畳む（消してからでは kill-session が届かない・実測 2026-09-10）。
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// 送達 ts より**前**の `SessionStart` しか足されない席（[`FakeStamp::Old`]・時計が戻った・古い hook の
/// 遅延書込）は、`/clear` の echo が pane に在っても作り直しと読まない（古い打刻を証拠に採らない）＝
/// `clear-unconfirmed`・復元を送らない。base は echo で `done` になる（RED）。
#[test]
fn seat_evidence_cycle_ignores_session_start_stamp_older_than_clear() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatevold";
    let log = dir.join("seat.log");
    let state = dir.join("state");
    let guard = start_clearing_seat(
        &socket, name, &log, &state_file(&seat_dir_of(&state, name)), (FakeStamp::Old, FakeStamp::Now),
    );
    assert!(guard.ready(), "古い打刻を置く偽の席を立てられる");
    let wm = dir.join("wm");
    wm_file(&wm, "working-memory.old.md", name);
    stamp_idle(&state, name);
    let (wm_s, state_s, rules_s) = (
        wm.display().to_string(),
        state.display().to_string(),
        fast_rules(&dir),
    );

    let out = run_seat(&[
        "cycle", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s, "--rules", &rules_s,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "stdout={}", stdout_of(&out));
    assert_eq!(stderr_of(&out), format!("seat: cycle failed reason=clear-unconfirmed{}\n", provenance(&state, "flag")));
    assert_eq!(fs::read_to_string(&log).unwrap_or_default(), "/clear\n", "古い打刻では復元を送らない");
    let pane = capture(&socket, name);
    assert!(pane.contains("/clear"), "echo は在る（字面は証拠ではない）: {pane}");
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// `/clear` の後に `SessionStart` を**打たない**席（hook が死んだ・載っていない）は、echo を描いても
/// 作り直しを確認できない＝`clear-unconfirmed`・復元を送らない（作り直しを確認できない席へ復元を
/// 刺さない・fail-closed）。送る**前**の最終行は `SessionStart`（Idle・前の作り直しの打刻）で、その
/// ts は未来（+5 s）＝「送達 ts 以後」に見える——**基線**（送る前に在った行は見ない）だけがこれを
/// 除外できる（lens-112 HIGH-1）。base は echo で `done` になる（RED）。
#[test]
fn seat_evidence_cycle_reports_clear_unconfirmed_when_no_session_start_stamp_follows() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatevnone";
    let log = dir.join("seat.log");
    let state = dir.join("state");
    let guard = start_clearing_seat(
        &socket, name, &log, &state_file(&seat_dir_of(&state, name)), (FakeStamp::Never, FakeStamp::Now),
    );
    assert!(guard.ready(), "打刻しない偽の席を立てられる");
    let wm = dir.join("wm");
    wm_file(&wm, "working-memory.none.md", name);
    let seat = seat_dir_of(&state, name);
    fs::create_dir_all(&seat).ok();
    fs::write(
        state_file(&seat),
        format!("{}\n", stamp_line("idle", "SessionStart", unix_now().saturating_add(5), "preexisting-future")),
    )
    .expect("打刻 fixture を置ける");
    let (wm_s, state_s, rules_s) = (
        wm.display().to_string(),
        state.display().to_string(),
        fast_rules(&dir),
    );

    let out = run_seat(&[
        "cycle", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s, "--rules", &rules_s,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "stdout={}", stdout_of(&out));
    assert_eq!(stderr_of(&out), format!("seat: cycle failed reason=clear-unconfirmed{}\n", provenance(&state, "flag")));
    assert_eq!(fs::read_to_string(&log).unwrap_or_default(), "/clear\n", "打刻が無い周は復元を送らない");
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// 送達の消費は **送達 ts 以後に足された `UserPromptSubmit` の打刻**で決める（設計 §6）。席は受けた行で
/// 打刻し、その後 prompt に打ちかけ（`❯ pending`）を残す＝「入力欄が空」で消費を読む実装（base）は
/// `consumed=false` になり（RED）、打刻で読む実装は `true`。
#[test]
fn seat_evidence_inject_consumed_true_by_user_prompt_submit_stamp_even_with_pending_input() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatevconsumed";
    let log = dir.join("seat.log");
    let state = dir.join("state");
    let stamps = state_file(&seat_dir_of(&state, name));
    stamp_idle(&state, name);
    let guard = start_clearing_seat_with(
        &socket,
        name,
        &log,
        ":",
        &format!(
            "{}; printf 'seat got %s\\n' \"$line\"; printf '\u{276f} pending'; sleep 30",
            stamp_cmd(&stamps, "busy", "UserPromptSubmit", FakeStamp::Now)
        ),
    );
    assert!(guard.ready(), "打刻して打ちかけを残す偽の席を立てられる");
    let payload = "seat-e2e-stamped";

    let out = run_seat(&[
        "inject", "--target", name, "--tmux-socket", &socket,
        "--state-dir", &state.display().to_string(), "--text", payload,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!(
            "seat: inject delivered target={name} bytes={} consumed=true{}\n",
            payload.len(),
            provenance(&state, "flag")
        ),
        "打ちかけが残っていても、送達 ts 以後の UserPromptSubmit 打刻で consumed=true"
    );
    let pane = capture(&socket, name);
    assert!(pane.contains("\u{276f} pending"), "入力欄は非空のまま: {pane}");
    assert!(
        fs::read_to_string(&stamps).unwrap_or_default().contains(r#""event":"UserPromptSubmit""#),
        "消費の打刻が在る"
    );
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// **送る前から在る**打刻しか無い席（`sh -i`＝受けた行で打刻しない・入力欄はすぐ空に戻る）は
/// `consumed=false`（queue の形）。fixture は `UserPromptSubmit` を 2 行置く: 100 秒前のものと、
/// **ts が送達 ts 以後に見える未来（+5 s）のもの**——後者は「送る前に在った行は見ない」基線だけが
/// 除外できる（`ts >= since` では拾ってしまう・lens-112 HIGH-1）。base は入力欄が空なので `true`（RED）。
#[test]
fn seat_evidence_inject_reports_consumed_false_when_only_older_stamps_exist() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatevolder";
    let guard = start_seat(&socket, name);
    assert!(guard.ready(), "独立 socket に prompt 付きの session を立てられる");
    let state = dir.join("state");
    // 送る前から在る打刻だけ（UserPromptSubmit 2 行・片方は ts が未来）＝送達後に足される行は無い。
    let seat = seat_dir_of(&state, name);
    fs::create_dir_all(&seat).ok();
    fs::write(
        state_file(&seat),
        format!(
            "{}\n{}\n",
            stamp_line("busy", "UserPromptSubmit", unix_now().saturating_sub(100), "old"),
            stamp_line("busy", "UserPromptSubmit", unix_now().saturating_add(5), "preexisting-future")
        ),
    )
    .expect("打刻 fixture を置ける");
    let payload = ": seat-e2e-older";

    let out = run_seat(&[
        "inject", "--target", name, "--tmux-socket", &socket,
        "--state-dir", &state.display().to_string(), "--text", payload,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!(
            "seat: inject delivered target={name} bytes={} consumed=false{}\n",
            payload.len(),
            provenance(&state, "flag")
        ),
        "送る前から在る打刻しか無い周は consumed=false（ts が未来でも基線より前の行は証拠にしない）"
    );
    assert_eq!(stderr_of(&out), "", "成功の周は stderr 0 行");
    let pane = capture(&socket, name);
    assert!(pane.contains("seat-e2e-older"), "字面は現れている（送達は成立）: {pane}");
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// 置き場が**解けない**周（`--state-dir` 無し・git の外の cwd）に送達した注入は、打刻の在処を知らない
/// ので `consumed=unknown reason=state-dir`（測れない・2 語は出さない・記録もしない）。3 つ目の理由の pin。
#[test]
fn seat_evidence_inject_reports_unknown_reason_state_dir_when_place_is_unresolved() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatevnodir";
    let guard = start_seat(&socket, name);
    assert!(guard.ready(), "独立 socket に prompt 付きの session を立てられる");
    let payload = ": seat-e2e-nodir";

    let out = run_seat_in(&dir, &["inject", "--target", name, "--tmux-socket", &socket, "--text", payload]);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!("seat: inject delivered target={name} bytes={} consumed=unknown reason=state-dir\n", payload.len()),
        "置き場が解けない周は unknown reason=state-dir（2 語なし）"
    );
    assert!(!dir.join("seat").exists(), "cwd に置き場を作らない");
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// `consumed=false`（queue）は**送達の成功**であって失敗ではない: tick は `decision=inject` rc 0 で自打刻し、
/// 次の周は brake（`pointer-recent`・`.109`）で**再送しない**（pointer は pane に 1 度だけ現れる）。false を失敗と読んで
/// 打刻を飛ばす実装は 2 周目にもう 1 本送ってここで落ちる（planner 裁定 2026-09-12 の条件）。
#[test]
fn seat_evidence_tick_treats_consumed_false_as_delivered_and_does_not_resend() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatevqueue";
    let guard = start_seat(&socket, name);
    assert!(guard.ready(), "独立 socket に prompt 付きの session を立てられる");
    let state = dir.join("state");
    let wm = dir.join("wm");
    fs::create_dir_all(&wm).ok();
    // 打刻 file は在る（Idle）が `sh -i` は submit で打刻しない＝送達した pointer は consumed=false になる形。
    stamp_idle(&state, name);
    let (wm_s, state_s) = (wm.display().to_string(), state.display().to_string());
    let args = [
        "tick", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s,
    ];

    let first = run_seat(&args);
    assert_eq!(rc_of(&first), i32::from(RC_OK), "stderr={}", stderr_of(&first));
    assert_eq!(tick_token(&stdout_of(&first), "decision").as_deref(), Some("inject"), "{}", stdout_of(&first));
    assert_eq!(tick_token(&stdout_of(&first), "consumed").as_deref(), Some("false"), "{}", stdout_of(&first));
    assert!(seat_dir_of(&state, name).join("tick-stamp").is_file(), "false でも送達は成立＝自打刻する");

    let second = run_seat(&args);
    assert_eq!(rc_of(&second), i32::from(RC_OK), "stderr={}", stderr_of(&second));
    assert_eq!(tick_token(&stdout_of(&second), "reason").as_deref(), Some("pointer-recent"), "2 周目は再送しない: {}", stdout_of(&second));
    let pointer = format!("seat heartbeat --target {name}");
    assert_eq!(capture(&socket, name).matches(&pointer).count(), 1, "pointer は 1 度だけ現れる");
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// cycle の復元が queue のまま（`UserPromptSubmit` の打刻が来ない）周は `restore-unconfirmed` と**記録する**
/// だけで、次の周の tick は back-off（`cycle-recent`・`.110`）で `/clear` も復元も**再送しない**。
/// `/clear` と `/rebrief` は偽の席の受信 log に 1 度ずつしか現れない。
#[test]
fn seat_evidence_cycle_does_not_resend_when_restore_stays_queued() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatevnoresend";
    let log = dir.join("seat.log");
    let state = dir.join("state");
    // `/clear` で SessionStart は打つが、復元の行では打刻しない（queue のまま）。
    let guard = start_clearing_seat(
        &socket, name, &log, &state_file(&seat_dir_of(&state, name)), (FakeStamp::Now, FakeStamp::Never),
    );
    assert!(guard.ready(), "復元を消費しない偽の席を立てられる");
    let wm = dir.join("wm");
    wm_file(&wm, "working-memory.noresend.md", name);
    stamp_idle(&state, name);
    let (wm_s, state_s, rules_s) = (
        wm.display().to_string(),
        state.display().to_string(),
        fast_rules(&dir),
    );
    let args = [
        "tick", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s, "--rules", &rules_s,
    ];

    let first = run_seat(&args);
    assert_eq!(rc_of(&first), i32::from(RC_OK), "stderr={}", stderr_of(&first));
    assert_eq!(
        tick_token(&stdout_of(&first), "cycle").as_deref(),
        Some("failed"),
        "復元の消費が確認できない周は failed（restore-unconfirmed）: {}",
        stdout_of(&first)
    );
    assert!(stdout_of(&first).contains(" reason=restore-unconfirmed"), "{}", stdout_of(&first));

    let second = run_seat(&args);
    assert_eq!(rc_of(&second), i32::from(RC_OK), "stderr={}", stderr_of(&second));
    assert_eq!(tick_token(&stdout_of(&second), "reason").as_deref(), Some("cycle-recent"), "2 周目は back-off: {}", stdout_of(&second));
    assert_eq!(
        fs::read_to_string(&log).unwrap_or_default(),
        "/clear\n/rebrief\n",
        "/clear も復元も 1 度ずつ（再送しない）"
    );
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

// ─────────────────── tick の口座の宣言は開いた rules から（`s2-07l.224`・接頭辞 `seat_tick_rules_accounts_`） ───────────────────

/// host の写しにだけ在る口座（tracked の a1〜a5 を含まない）。
const HOST_LABELS: &[&str] = &["hostonly", "hostonly2"];
/// 登録 row の口座（写しの 1 つ目）。
const HOST_SEAT: &str = "hostonly";
/// 立て直しの候補（写しの 2 つ目）。
const HOST_SPARE: &str = "hostonly2";

/// 口座の歯の置き場（`--rules` の写しの `[[account]]` は [`HOST_LABELS`]）。
fn host_place() -> AcctPlace {
    AcctPlace { labels: HOST_LABELS, ..acct_place() }
}

/// (a) `--rules` の写しにだけ在る口座（hostonly = 100・hostonly2 = 30）の席が退避して止まり、pane が shell の周は、
/// 写しの宣言から選んだ `hostonly2` で立て直す（`relaunch=hostonly2`・穴は hostonly2 の credential dir）。base は
/// 埋め込みの a1〜a5 から選び `relaunch=none:unmeasured`（RED）。
#[test]
fn seat_tick_rules_accounts_relaunch_selects_a_host_only_account() {
    let place = host_place();
    let name = "hostrelaunch";
    let guard = start_seat(&place.socket, name);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");
    let registered = acct_register_as(&place, name, HOST_SEAT, &acct_launcher(&place, name));
    assert_eq!(rc_of(&registered), i32::from(RC_OK), "stderr={}", stderr_of(&registered));
    acct_measured(&place.state, HOST_SEAT, 100, &acct_now());
    acct_measured(&place.state, HOST_SPARE, 30, &acct_now());
    let first = acct_signal(&place, name);
    assert_eq!(tick_token(&first, "kind").as_deref(), Some("externalize"), "1 周目は退避の合図: {first}");
    acct_stop(&place, name, unix_now().saturating_add(1));
    assert!(acct_shell_prompt(&place, name, ""), "shell の prompt を描ける");

    let out = acct_tick(&place, name, None);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(&out));
    for (key, want) in [("decision", "inject"), ("kind", "relaunch"), ("relaunch", HOST_SPARE)] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    assert_eq!(
        fs::read_to_string(place.dir.join("launched")).unwrap_or_default(),
        format!("{}\n", place.state.join("accounts").join(HOST_SPARE).display()),
        "穴は写しから選んだ口座の credential dir で埋まる"
    );
    let rows = acct_rows(&place.state);
    assert_eq!(rows.last().map(|row| row.account.as_str()), Some(HOST_SPARE), "登録 row の口座が更新される: {rows:?}");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (b) `--rules` の写しにだけ在る口座の row に実測行が無い周は、tick が同じ `--rules` で計測を 1 回撃ち（偽 curl の
/// 呼出 1 回）、その label の `AllowanceMeasured` が積まれて判定行は `account=hostonly:50`。base は宣言外として撃たず
/// `account=hostonly:unmeasured`（RED）。
#[test]
fn seat_tick_rules_accounts_measures_a_host_only_account() {
    use vessel::fleet::{Allowance, EventKind};
    let place = host_place();
    let name = "hostmeasure";
    let registered = acct_register_as(&place, name, HOST_SEAT, ACCT_LAUNCH);
    assert_eq!(rc_of(&registered), i32::from(RC_OK), "stderr={}", stderr_of(&registered));
    acct_credential(&place, HOST_SEAT);
    acct_fake_curl(&place, 50);
    write_state(&seat_dir_of(&place.state, name), StateFix::Busy { age_s: 0 });
    let pane = fixture(&place.dir, "pane.txt", IDLE_PANE);

    let (out, touched) = acct_tick_probed(&place, name, &pane);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        acct_line("decision=noop reason=busy", &format!("{ST_BUSY} account={HOST_SEAT}:50"), &place.state)
    );
    assert!(!touched, "注入しない");
    assert_eq!(acct_curl_calls(&place), 1, "写しの口座のために計測を 1 回撃つ");
    let measured = vessel::fleet::store::read_all(&place.state)
        .unwrap_or_default()
        .into_iter()
        .filter(|event| event.kind == EventKind::AllowanceMeasured)
        .filter_map(|event| event.allowance)
        .filter(|row| matches!(row, Allowance::Measured(found) if found.account == HOST_SEAT))
        .count();
    assert!(measured > 0, "AllowanceMeasured が写しの label で積まれる");
    fs::remove_dir_all(&place.dir).ok();
}

/// (c) `--rules` 無しの周は置き場の host の面（`host.toml`）の宣言で判定する（埋め込みの宣言に依らない）: a1 の row の
/// 古い実測行（95）は計測を 1 回撃って新しい行（50）で読み直す（偽 curl の呼出 1 回・判定行は `account=a1:50`）。
#[test]
fn seat_tick_rules_accounts_without_rules_uses_the_host_manifest_accounts() {
    let place = acct_place();
    let name = "hostaccts";
    let registered = acct_register(&place, name, ACCT_LAUNCH);
    assert_eq!(rc_of(&registered), i32::from(RC_OK), "stderr={}", stderr_of(&registered));
    let host: String = place.labels.iter().map(|label| format!("\n[[account]]\nlabel = \"{label}\"\n")).collect();
    fs::write(place.state.join(vessel::rules::HOST_MANIFEST), format!("schema = 1\n{host}")).expect("host の面を書ける");
    let stale = vessel::fleet::cli::format_utc(unix_now().saturating_sub(STALE_S + 600));
    acct_measured(&place.state, ACCT_SEAT, 95, &stale);
    acct_credential(&place, ACCT_SEAT);
    acct_fake_curl(&place, 50);
    write_state(&seat_dir_of(&place.state, name), StateFix::Busy { age_s: 0 });
    let pane = fixture(&place.dir, "pane.txt", IDLE_PANE);
    let state = place.state.display().to_string();
    let wm = place.wm.display().to_string();

    let (out, touched) = run_seat_probed(
        &place.dir,
        &["tick", "--target", name, "--wm-dir", &wm, "--tmux-socket", &place.socket, "--state-dir", &state, "--capture-file", &pane],
    );

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(&out));
    for (key, want) in [("decision", "noop"), ("reason", "busy"), ("account", "a1:50")] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    assert!(!touched, "注入しない");
    assert_eq!(acct_curl_calls(&place), 1, "host の面の宣言の口座を計測する");
    fs::remove_dir_all(&place.dir).ok();
}

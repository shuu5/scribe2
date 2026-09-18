//! 作業記憶の歯（退避 `seat externalize` / 消費 `seat consume` / 復元 `seat rebrief` / memo の棚卸し・
//! 設計 docs/design/working-memory.md §5 / §8・接頭辞 `seat_wm_` と `seat_rebrief_`）。
//!
//! 共有の helper と fixture は親 module（`tests/e2e/seat.rs`）に在り、`use super::*` で使う。
//! 歯の本文は `seat.rs` から**挙動不変で移した**もの（`s2-07l.261`）。
// flip-check: moved s2-07l.261

use super::*;

// ─────────────────── 作業記憶の退避（externalize・設計 working-memory.md §5.1 / §8） ───────────────────

/// 退避の歯の席（`:` を含む＝置き場の dir 名は潰した `wm_1`、frontmatter の `seat:` は逐語）。
const WM_TARGET: &str = "wm:1";
/// 上の target を潰した dir 名（契約の字面から組む）。
const WM_SEAT_DIR: &str = "wm_1";
/// 節 1 の見出し（設計 §3 の固定字面を歯の側でも逐語で持つ）。
const WM_HEAD_USER: &str = "## user 直命（verbatim・言い換え禁止）";
/// 節 2 の見出し。
const WM_HEAD_PLAN: &str = "## 計画弧・次のステップ";
/// 節 3 の見出し。
const WM_HEAD_DIRECTIVES: &str = "## この effort を貫く命令・制約";

/// 退避の歯の場所（tmp の wm dir・state dir・anchor・入力 file の dir）。
struct WmPlace {
    /// tmp の root。
    dir: PathBuf,
    /// 退避物の dir。
    wm: PathBuf,
    /// 置き場。
    state: PathBuf,
    /// 実在検査の repo root（fixture）。
    anchor: PathBuf,
}

/// 場所を作る。anchor には憲法 `n2` / `c11`・ADR-0018・設計 doc 1 本・repo 内 file 1 本・台帳 prefix `s2` を置く。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn wm_place() -> WmPlace {
    let dir = tmp();
    let (wm, state, anchor) = (dir.join("wm"), dir.join("state"), dir.join("anchor"));
    for sub in ["design-intent/spec", "design-intent/decisions", "docs/design", "src", ".beads"] {
        fs::create_dir_all(anchor.join(sub)).expect("anchor の dir を作れる");
    }
    fs::create_dir_all(&wm).expect("wm dir を作れる");
    fs::create_dir_all(dir.join("in")).expect("入力 dir を作れる");
    let files = [
        ("design-intent/spec/constitution.html", "<section id=\"n2\"></section>\n<section id=\"c11\"></section>\n"),
        ("design-intent/decisions/ADR-0018-working-memory.html", "<html></html>\n"),
        ("docs/design/working-memory.md", "# 設計\n"),
        ("src/lib.rs", "// fixture\n"),
        (".beads/metadata.json", "{\n  \"dolt_database\": \"s2\"\n}\n"),
    ];
    for (path, body) in files {
        fs::write(anchor.join(path), body).expect("anchor の file を書ける");
    }
    WmPlace { dir, wm, state, anchor }
}

/// 打刻を置く（行は契約の字面から組む・最後の行が現在の sid）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn wm_stamp(place: &WmPlace, sids: &[&str]) {
    let seat = seat_dir_of(&place.state, WM_SEAT_DIR);
    fs::create_dir_all(&seat).expect("seat dir を作れる");
    let lines: String = sids
        .iter()
        .map(|sid| format!("{}\n", stamp_line("idle", "Stop", unix_now(), sid)))
        .collect();
    fs::write(state_file(&seat), lines).expect("打刻を置ける");
}

/// 上限だけを持つ rules の fixture を書き、`--rules` に渡す path を返す。
fn wm_rules(place: &WmPlace, cap: u64) -> String {
    fixture(
        &place.dir,
        "wm-rules.toml",
        &format!(
            "schema = 1\n\n[[rule]]\nid = \"seat.wm_directive_cap\"\nkind = \"WmDirectiveCap\"\nvalue = {cap}\n\
             enabled = true\nruling = \"user 2026-09-12T02:01Z\"\nruled_at = \"2026-09-12\"\n"
        ),
    )
}

/// `seat externalize` を 1 回撃つ（計画弧は固定・節 3 の新規行は `directives`・`extra` は追加の flag）。
fn wm_externalize(place: &WmPlace, directives: &str, extra: &[&str]) -> Output {
    let input = place.dir.join("in");
    let plan = fixture(&input, "plan.md", "- 次は s2-07l.139 の land\n");
    let directives = fixture(&input, "directives.md", directives);
    let (wm, state, anchor) = (
        place.wm.display().to_string(),
        place.state.display().to_string(),
        place.anchor.display().to_string(),
    );
    let mut args = vec![
        "externalize", "--target", WM_TARGET, "--wm-dir", &wm, "--state-dir", &state, "--anchor", &anchor,
        "--plan", &plan, "--directives", &directives,
    ];
    args.extend_from_slice(extra);
    run_seat(&args)
}

/// wm dir に在る退避物の名前（sort 済み）。
fn wm_names(place: &WmPlace) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(&place.wm)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// 消費済み退避物を 1 つ置く（`front` は frontmatter の中身・節は逐語）。
fn wm_consumed(place: &WmPlace, name: &str, front: &str, user: &str, directives: &str) -> PathBuf {
    let path = place.wm.join(name);
    let body = format!(
        "---\n{front}---\n\n{WM_HEAD_USER}\n{user}\n{WM_HEAD_PLAN}\n- 前の計画\n\n{WM_HEAD_DIRECTIVES}\n{directives}"
    );
    fs::write(&path, body).ok();
    path
}

/// 節 3 の本文だけを返す（見出しの後ろ全部）。
fn wm_directive_section(text: &str) -> String {
    text.split_once(WM_HEAD_DIRECTIVES)
        .map(|(_, tail)| tail.to_owned())
        .unwrap_or_default()
}

/// (1) 打刻の最終行の sid で file 名が決まる。打刻が無い周は rc 1 `sid-missing` で理由を stderr に出し、file を作らない。
#[test]
fn seat_wm_externalize_names_file_by_stamped_sid_and_refuses_without_stamp() {
    let place = wm_place();
    let line = "- [auto] [P1] since=2026-09-13 退避は器の口で → SSOT: ADR-0018 §2.3\n";
    let missing = wm_externalize(&place, line, &[]);
    assert_eq!(rc_of(&missing), i32::from(RC_REFUSED), "打刻が無い周は rc 1: {}", stdout_of(&missing));
    assert!(stderr_of(&missing).contains("reason=sid-missing"), "理由: {}", stderr_of(&missing));
    assert!(stdout_of(&missing).is_empty(), "断る周は stdout に書かない");
    assert!(wm_names(&place).is_empty(), "file を作らない: {:?}", wm_names(&place));

    wm_stamp(&place, &["sid-old", "sid-now-1"]);
    let out = wm_externalize(&place, line, &[]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        "seat: externalized file=working-memory.sid-now-1.md carried=0 dropped_provisional=0 dropped_unresolved=0 dropped_retired=0 directives=1\n",
        "stdout 1 行"
    );
    assert_eq!(wm_names(&place), vec!["working-memory.sid-now-1.md".to_owned()], "最終行の sid の名義で 1 つだけ");
    fs::remove_dir_all(&place.dir).ok();
}

/// (2) 自席の未 consumed 退避物が在れば rc 1 `wm-exists` で新しい file を作らない（他席の未 consumed は止めない）。
#[test]
fn seat_wm_externalize_refuses_when_own_unconsumed_wm_exists() {
    let place = wm_place();
    wm_stamp(&place, &["sid-2"]);
    let other = wm_file(&place.wm, "working-memory.other.md", "other:1");
    let first = wm_externalize(&place, "", &[]);
    assert_eq!(rc_of(&first), i32::from(RC_OK), "他席の未 consumed は止めない: {}", stderr_of(&first));
    fs::remove_file(place.wm.join("working-memory.sid-2.md")).ok();

    let own = wm_file(&place.wm, "working-memory.prev.md", WM_TARGET);
    let before = fs::read_to_string(&own).unwrap_or_default();
    let out = wm_externalize(&place, "", &[]);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "stdout={}", stdout_of(&out));
    assert!(stderr_of(&out).contains("reason=wm-exists"), "理由: {}", stderr_of(&out));
    assert!(!place.wm.join("working-memory.sid-2.md").exists(), "新しい file を作らない");
    assert_eq!(fs::read_to_string(&own).unwrap_or_default(), before, "既存の退避物は不変");
    assert!(other.exists(), "他席の退避物は不変");
    fs::remove_dir_all(&place.dir).ok();
}

/// carry 元の節 3（pointer 行 3 本〔repo path P2・憲法 P0・ADR P1〕と暫定行 2 本〔SSOT 無し・user 裁定だけ〕）。
const CARRY_DIRECTIVES: &str = concat!(
    "- [auto] [P2] since=2026-09-01 repo の現物 → SSOT: src/lib.rs\n",
    "- [confirm] [P1] since=2026-09-01 矢印の無い命令\n",
    "- [auto] [P0] since=2026-09-01 prose は規則でない → SSOT: 憲法 N2\n",
    "- [auto] [P1] since=2026-09-01 裁定だけの命令 → SSOT: user 裁定 2026-09-12T02:01Z\n",
    "- [hard候補] [P1] since=2026-09-01 退避は器の口 → SSOT: ADR-0018 §2.3\n",
);

/// carry 元の節 1（未着手・完了・着手中〔従属行つき〕・user 撤回）。
const CARRY_USER: &str = concat!(
    "- [2026-09-12 10:00] 「A を直せ」 → 状態: 未着手\n",
    "- [2026-09-12 10:05] 「B は済んだ」 → 状態: 完了 s2-07l.1\n",
    "- [2026-09-12 10:10] 「C を、そのまま」 → 状態: 着手中 s2-07l.2\n",
    "  補足の従属行（逐語）\n",
    "- [2026-09-12 10:20] 「D はやめる」 → 状態: user 撤回\n",
);

/// (3) consumed からの carry で pointer 行 3 本は残り暫定行 2 本は落ちる・残りは P 昇順の安定 sort。
#[test]
fn seat_wm_externalize_carries_pointer_lines_and_drops_provisional_in_priority_order() {
    let place = wm_place();
    wm_stamp(&place, &["sid-3"]);
    wm_consumed(&place, "working-memory.sid-2.consumed.md", &format!("schema: 1\nseat: {WM_TARGET}\n"), CARRY_USER, CARRY_DIRECTIVES);
    let fresh = "- [confirm] [P1] since=2026-09-13 新規の命令 → SSOT: docs/design/working-memory.md §5.1\n";
    let out = wm_externalize(&place, fresh, &[]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        "seat: externalized file=working-memory.sid-3.md carried=3 dropped_provisional=2 dropped_unresolved=0 dropped_retired=0 directives=1\n"
    );
    let text = fs::read_to_string(place.wm.join("working-memory.sid-3.md")).unwrap_or_default();
    let section = wm_directive_section(&text);
    let lines: Vec<&str> = section.lines().filter(|line| line.starts_with("- ")).collect();
    assert_eq!(
        lines,
        vec![
            "- [auto] [P0] since=2026-09-01 prose は規則でない → SSOT: 憲法 N2",
            "- [hard候補] [P1] since=2026-09-01 退避は器の口 → SSOT: ADR-0018 §2.3",
            "- [confirm] [P1] since=2026-09-13 新規の命令 → SSOT: docs/design/working-memory.md §5.1",
            "- [auto] [P2] since=2026-09-01 repo の現物 → SSOT: src/lib.rs",
        ],
        "P 昇順・同じ P は carry → 新規の順（安定）: {text}"
    );
    assert!(!text.contains("矢印の無い命令") && !text.contains("裁定だけの命令"), "暫定行は運ばない: {text}");
    assert!(text.contains("\ncarry_source: working-memory.sid-2.consumed.md\n"), "{text}");
    assert!(text.contains("\ncarry_items: 3\n"), "{text}");
    fs::remove_dir_all(&place.dir).ok();
}

/// (4) 節 1 は逐語で全行残り「完了」「user 撤回」の行だけ落ちる・`--user` の追記は逐語で後ろに足す。
#[test]
fn seat_wm_externalize_carries_user_section_verbatim_and_drops_only_closed_rows() {
    let place = wm_place();
    wm_stamp(&place, &["sid-4"]);
    wm_consumed(&place, "working-memory.sid-3.consumed.md", &format!("seat: {WM_TARGET}\n"), CARRY_USER, "");
    let user = fixture(&place.dir, "user.md", "- [2026-09-13 01:00] 「E、語尾も逐語で。」 → 状態: 未着手\n");
    let out = wm_externalize(&place, "", &["--user", &user]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    let text = fs::read_to_string(place.wm.join("working-memory.sid-4.md")).unwrap_or_default();
    let section = text
        .split_once(WM_HEAD_USER)
        .and_then(|(_, tail)| tail.split_once(WM_HEAD_PLAN))
        .map(|(body, _)| body.trim_matches('\n').to_owned())
        .unwrap_or_default();
    assert_eq!(
        section,
        concat!(
            "- [2026-09-12 10:00] 「A を直せ」 → 状態: 未着手\n",
            "- [2026-09-12 10:10] 「C を、そのまま」 → 状態: 着手中 s2-07l.2\n",
            "  補足の従属行（逐語）\n",
            "- [2026-09-13 01:00] 「E、語尾も逐語で。」 → 状態: 未着手",
        ),
        "逐語・閉じた 2 行だけ落ちる: {text}"
    );
    assert!(text.contains("\ncarry_user_directives: 2\n"), "{text}");
    assert!(text.contains(&format!("{WM_HEAD_PLAN}\n- 次は s2-07l.139 の land\n")), "計画弧は --plan の逐語: {text}");
    fs::remove_dir_all(&place.dir).ok();
}

/// (5) `--directives` の tag / P / since 欠落の 3 行は行番号付きで全件 stderr・rc 1・file 不作成。SSOT 欠落の行は断りに載らない。
#[test]
fn seat_wm_externalize_reports_every_grammar_failure_with_line_numbers() {
    let place = wm_place();
    wm_stamp(&place, &["sid-5"]);
    let lines = concat!(
        "- [P1] since=2026-09-13 tag 欠落 → SSOT: 憲法 N2\n",
        "<!-- テンプレの説明（捨てる） -->\n",
        "- [auto] since=2026-09-13 P 欠落 → SSOT: 憲法 N2\n",
        "- [auto] [P1] since 欠落 → SSOT: 憲法 N2\n",
        "- [confirm] [P2] since=2026-09-13 SSOT 欠落（暫定行として入る形）\n",
    );
    let out = wm_externalize(&place, lines, &[]);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "stdout={}", stdout_of(&out));
    let err = stderr_of(&out);
    assert_eq!(
        err,
        concat!(
            "seat: externalize refused reason=directive-grammar lines=3\n",
            "seat: externalize directive line=1 missing=tag\n",
            "seat: externalize directive line=3 missing=priority\n",
            "seat: externalize directive line=4 missing=since\n",
        ),
        "全件・行番号付き（コメント行も行番号を数える）"
    );
    assert!(wm_names(&place).is_empty(), "file を作らない: {:?}", wm_names(&place));
    // SSOT 欠落だけの行は止めない（暫定行として入る）。
    let provisional = wm_externalize(&place, "- [confirm] [P2] since=2026-09-13 SSOT 欠落\n", &[]);
    assert_eq!(rc_of(&provisional), i32::from(RC_OK), "stderr={}", stderr_of(&provisional));
    fs::remove_dir_all(&place.dir).ok();
}

/// (6) 節 3 の合計が上限（fixture の rules で 3）を超えたら rc 1 `directive-cap`（黙って切らない）・ちょうど 3 は通る。
#[test]
fn seat_wm_externalize_refuses_over_directive_cap_from_rules() {
    let place = wm_place();
    wm_stamp(&place, &["sid-6"]);
    let rules = wm_rules(&place, 3);
    let row = |n: u32| format!("- [auto] [P1] since=2026-09-13 命令 {n} → SSOT: 憲法 N2\n");
    let four: String = (1..=4).map(row).collect();
    let over = wm_externalize(&place, &four, &["--rules", &rules]);
    assert_eq!(rc_of(&over), i32::from(RC_REFUSED), "stdout={}", stdout_of(&over));
    assert_eq!(stderr_of(&over), "seat: externalize refused reason=directive-cap total=4 cap=3\n");
    assert!(wm_names(&place).is_empty(), "file を作らない: {:?}", wm_names(&place));
    let three: String = (1..=3).map(row).collect();
    let at = wm_externalize(&place, &three, &["--rules", &rules]);
    assert_eq!(rc_of(&at), i32::from(RC_OK), "上限ちょうどは通る: {}", stderr_of(&at));
    fs::remove_dir_all(&place.dir).ok();
}

/// (7) frontmatter に `schema: 1` と `seat:` が在り、`--trigger` と `--role` が写る（既定の trigger は manual）。
#[test]
fn seat_wm_externalize_writes_frontmatter_with_schema_seat_and_trigger() {
    let place = wm_place();
    wm_stamp(&place, &["sid-7"]);
    let out = wm_externalize(&place, "", &["--trigger", "tick", "--role", "orchestrator"]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    let text = fs::read_to_string(place.wm.join("working-memory.sid-7.md")).unwrap_or_default();
    assert!(text.starts_with(&format!("---\nschema: 1\nseat: {WM_TARGET}\nrole: orchestrator\nexternalized_at: ")), "{text}");
    for key in ["\ntrigger: tick\n", "\ncarry_source: none\n", "\ncarry_items: 0\n", "\ncarry_user_directives: 0\n"] {
        assert!(text.contains(key), "{key:?} が在る: {text}");
    }
    for head in [WM_HEAD_USER, WM_HEAD_PLAN, WM_HEAD_DIRECTIVES] {
        assert!(text.contains(&format!("\n{head}\n")), "見出し {head}: {text}");
    }
    fs::remove_dir_all(&place.wm).ok();
    fs::create_dir_all(&place.wm).ok();
    let manual = wm_externalize(&place, "", &[]);
    assert_eq!(rc_of(&manual), i32::from(RC_OK), "stderr={}", stderr_of(&manual));
    let text = fs::read_to_string(place.wm.join("working-memory.sid-7.md")).unwrap_or_default();
    assert!(text.contains("\ntrigger: manual\n") && !text.contains("\nrole:"), "既定: {text}");
    let bad = wm_externalize(&place, "", &["--trigger", "cron"]);
    assert_eq!(rc_of(&bad), i32::from(RC_REFUSED), "未知の trigger は使い方の誤り");
    fs::remove_dir_all(&place.dir).ok();
}

/// (8) 他席の consumed は carry の source にならない・unresolved の行（実在しない憲法 id）は落ちて数えられる・
/// `schema` 無しの consumed も carry 元になる・打刻の sid が空なら rc 1 `sid-empty`。
#[test]
fn seat_wm_externalize_carries_only_own_seat_and_drops_unresolved_rows() {
    let place = wm_place();
    let own = wm_consumed(
        &place,
        "working-memory.sid-a.consumed.md",
        &format!("seat: {WM_TARGET}\n"),
        "",
        concat!(
            "- [auto] [P1] since=2026-09-01 在る条 → SSOT: 憲法 C11.2\n",
            "- [auto] [P0] since=2026-09-01 無い条 → SSOT: 憲法 C99 / s2-07l.61\n",
        ),
    );
    backdate(&own, 600);
    wm_consumed(
        &place,
        "working-memory.sid-b.consumed.md",
        "schema: 1\nseat: other:1\n",
        "",
        "- [auto] [P0] since=2026-09-01 他席の命令 → SSOT: 憲法 N2\n",
    );
    wm_stamp(&place, &["sid-9", ""]);
    let empty = wm_externalize(&place, "", &[]);
    assert_eq!(rc_of(&empty), i32::from(RC_REFUSED), "stdout={}", stdout_of(&empty));
    assert!(stderr_of(&empty).contains("reason=sid-empty"), "理由: {}", stderr_of(&empty));

    wm_stamp(&place, &["sid-9"]);
    let out = wm_externalize(&place, "", &[]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        "seat: externalized file=working-memory.sid-9.md carried=1 dropped_provisional=0 dropped_unresolved=1 dropped_retired=0 directives=0\n"
    );
    let text = fs::read_to_string(place.wm.join("working-memory.sid-9.md")).unwrap_or_default();
    assert!(text.contains("\ncarry_source: working-memory.sid-a.consumed.md\n"), "新しい他席の file は source にならない: {text}");
    assert!(text.contains("在る条") && !text.contains("無い条") && !text.contains("他席の命令"), "{text}");
    fs::remove_dir_all(&place.dir).ok();
}

/// 退役の歯の carry 元の節 3（Resolved の pointer 行 3 本）。
const RETIRE_CARRY: &str = concat!(
    "- [auto] [P0] since=2026-09-01 prose は規則でない → SSOT: 憲法 N2\n",
    "- [hard候補] [P1] since=2026-09-01 退避は器の口 → SSOT: ADR-0018 §2.3\n",
    "- [auto] [P2] since=2026-09-01 repo の現物 → SSOT: src/lib.rs\n",
);

/// (d-1) `--retire` の行と全文一致した carry 元の行だけ落ち、`dropped_retired` に数えられる（空行・コメントは捨てる）。
#[test]
fn seat_wm_externalize_retires_matching_directive_rows_and_counts_them() {
    let place = wm_place();
    wm_stamp(&place, &["sid-r1"]);
    wm_consumed(&place, "working-memory.sid-r0.consumed.md", &format!("seat: {WM_TARGET}\n"), "", RETIRE_CARRY);
    let retire = fixture(
        &place.dir,
        "retire.md",
        "<!-- 役目を終えた行 -->\n\n- [hard候補] [P1] since=2026-09-01 退避は器の口 → SSOT: ADR-0018 §2.3   \n",
    );
    let out = wm_externalize(&place, "", &["--retire", &retire]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        "seat: externalized file=working-memory.sid-r1.md carried=2 dropped_provisional=0 dropped_unresolved=0 dropped_retired=1 directives=0\n"
    );
    let text = fs::read_to_string(place.wm.join("working-memory.sid-r1.md")).unwrap_or_default();
    let section = wm_directive_section(&text);
    let lines: Vec<&str> = section.lines().filter(|line| line.starts_with("- ")).collect();
    assert_eq!(
        lines,
        vec![
            "- [auto] [P0] since=2026-09-01 prose は規則でない → SSOT: 憲法 N2",
            "- [auto] [P2] since=2026-09-01 repo の現物 → SSOT: src/lib.rs",
        ],
        "一致した 1 行だけ落ちる: {text}"
    );
    assert!(text.contains("\ncarry_items: 2\n"), "{text}");
    fs::remove_dir_all(&place.dir).ok();
}

/// (d-2) carry 元に一致しない行（部分一致・暫定行として落ちる行を含む）が在れば rc 1 `retire-unmatched`・行番号付きで全件・退避物を作らない。
#[test]
fn seat_wm_externalize_refuses_unmatched_retire_row() {
    let place = wm_place();
    wm_stamp(&place, &["sid-r2"]);
    let carry = format!("{RETIRE_CARRY}- [confirm] [P1] since=2026-09-01 矢印の無い命令\n");
    wm_consumed(&place, "working-memory.sid-r0.consumed.md", &format!("seat: {WM_TARGET}\n"), "", &carry);
    let retire = fixture(
        &place.dir,
        "retire.md",
        concat!(
            "- [auto] [P0] since=2026-09-01 prose は規則でない → SSOT: 憲法 N2\n",
            "- [auto] [P2] since=2026-09-01 repo の現物\n",
            "- [confirm] [P1] since=2026-09-01 矢印の無い命令\n",
            "- [auto] [P0] since=2026-09-01 prose は規則でない → SSOT: 憲法 N2\n",
        ),
    );
    let out = wm_externalize(&place, "", &["--retire", &retire]);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "stdout={}", stdout_of(&out));
    assert_eq!(
        stderr_of(&out),
        concat!(
            "seat: externalize refused reason=retire-unmatched lines=3\n",
            "seat: externalize retire line=2 unmatched\n",
            "seat: externalize retire line=3 unmatched\n",
            "seat: externalize retire line=4 unmatched\n",
        ),
        "部分一致・落ちる暫定行・同じ行の 2 本目は一致しない"
    );
    assert!(stdout_of(&out).is_empty(), "断る周は stdout に書かない");
    assert!(!place.wm.join("working-memory.sid-r2.md").exists(), "退避物を作らない: {:?}", wm_names(&place));
    let absent = wm_externalize(&place, "", &["--retire", &place.dir.join("none.md").display().to_string()]);
    assert!(stderr_of(&absent).contains("reason=input-unreadable flag=--retire"), "{}", stderr_of(&absent));
    fs::remove_dir_all(&place.dir).ok();
}

/// (d-3) cap ちょうどの carry 元から 1 行退役すれば新規 1 行を足せる（退役は cap の検査より先に効く）。
#[test]
fn seat_wm_externalize_retire_lets_cap_admit_a_new_row() {
    let place = wm_place();
    wm_stamp(&place, &["sid-r3"]);
    wm_consumed(&place, "working-memory.sid-r0.consumed.md", &format!("seat: {WM_TARGET}\n"), "", RETIRE_CARRY);
    let rules = wm_rules(&place, 3);
    let fresh = "- [confirm] [P1] since=2026-09-14 新規の命令 → SSOT: docs/design/working-memory.md §5.1\n";
    let over = wm_externalize(&place, fresh, &["--rules", &rules]);
    assert_eq!(stderr_of(&over), "seat: externalize refused reason=directive-cap total=4 cap=3\n", "退役無しは上限超え");
    let retire = fixture(&place.dir, "retire.md", "- [auto] [P2] since=2026-09-01 repo の現物 → SSOT: src/lib.rs\n");
    let out = wm_externalize(&place, fresh, &["--rules", &rules, "--retire", &retire]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        "seat: externalized file=working-memory.sid-r3.md carried=2 dropped_provisional=0 dropped_unresolved=0 dropped_retired=1 directives=1\n"
    );
    fs::remove_dir_all(&place.dir).ok();
}

/// (9) 実在検査の 3 値: 憲法 id 在 → Resolved / 無 → Unresolved / 台帳 → Unchecked（ADR・設計・repo path・anchor 外も）。
#[test]
fn seat_wm_pointer_resolution_is_three_valued_against_the_anchor() {
    use vessel::seat::wm::{classify, Anchor, PointerKind, Resolution};
    let place = wm_place();
    let anchor = Anchor::open(&place.anchor).expect("anchor を開ける");
    assert_eq!(anchor.prefixes(), ["s2".to_owned()], "台帳 prefix は .beads から解く");
    let cases = [
        ("憲法 N2", PointerKind::Constitution, Resolution::Resolved),
        ("C11.2", PointerKind::Constitution, Resolution::Resolved),
        ("憲法 C99", PointerKind::Constitution, Resolution::Unresolved),
        ("ADR-0018 §2.2", PointerKind::Adr, Resolution::Resolved),
        ("ADR-0099", PointerKind::Adr, Resolution::Unresolved),
        ("docs/design/working-memory.md §4", PointerKind::Design, Resolution::Resolved),
        ("docs/design/missing.md", PointerKind::Design, Resolution::Unresolved),
        ("src/lib.rs#fixture", PointerKind::RepoPath, Resolution::Resolved),
        ("../anchor/src/lib.rs", PointerKind::RepoPath, Resolution::Unresolved),
        ("rules 行 seat.wm_directive_cap", PointerKind::Manifest, Resolution::Unresolved),
        ("s2-07l.61", PointerKind::Ledger, Resolution::Unchecked),
        ("auto-memory some-slug", PointerKind::Memory, Resolution::Unchecked),
        ("PR #128", PointerKind::PullRequest, Resolution::Unchecked),
    ];
    for (reference, kind, want) in cases {
        assert_eq!(classify(reference, anchor.prefixes()), Some(kind), "{reference}");
        assert_eq!(anchor.resolve(kind, reference), want, "{reference}");
    }
    fs::create_dir_all(place.anchor.join("rules")).ok();
    fs::write(place.anchor.join("rules/manifest.toml"), fs::read_to_string(wm_rules(&place, 24)).unwrap_or_default()).ok();
    assert_eq!(anchor.resolve(PointerKind::Manifest, "rules 行 seat.wm_directive_cap"), Resolution::Resolved, "manifest の行 id");
    fs::remove_dir_all(&place.dir).ok();
}

// ─────────────────── 作業記憶の消費（consume・設計 working-memory.md §5.3 / §9 契約 (c)） ───────────────────

/// `seat consume` を 1 回撃つ。
fn wm_consume(place: &WmPlace) -> Output {
    let (wm, state) = (place.wm.display().to_string(), place.state.display().to_string());
    run_seat(&["consume", "--target", WM_TARGET, "--wm-dir", &wm, "--state-dir", &state])
}

/// 退避物を全文逐語で置く。
fn wm_raw(place: &WmPlace, name: &str, text: &str) -> PathBuf {
    let path = place.wm.join(name);
    fs::write(&path, text).ok();
    path
}

/// consume の歯の退避物（frontmatter に `---` の行を本文にも持つ＝閉じ区切りの位置を取り違えると本文が変わる）。
fn wm_body(seat: &str) -> String {
    format!("---\nschema: 1\nseat: {seat}\ntrigger: manual\n---\n\n## 計画弧・次のステップ\n- 続き\n---\n末尾の行\n")
}

/// (1) 同 sid: `working-memory.<sid>.consumed.md` へ rename・内容は 1 byte も不変・元 file は不在。
#[test]
fn seat_wm_consume_renames_same_sid_without_changing_a_byte() {
    let place = wm_place();
    wm_stamp(&place, &["sid-old", "sid-5"]);
    let text = wm_body(WM_TARGET);
    let source = wm_raw(&place, "working-memory.sid-5.md", &text);
    let out = wm_consume(&place);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), "seat: consumed file=working-memory.sid-5.consumed.md\n", "stdout 1 行");
    assert!(stderr_of(&out).is_empty(), "stderr は空: {}", stderr_of(&out));
    assert!(!source.exists(), "元 file は不在");
    assert_eq!(wm_names(&place), vec!["working-memory.sid-5.consumed.md".to_owned()], "move だけ");
    let moved = fs::read(place.wm.join("working-memory.sid-5.consumed.md")).unwrap_or_default();
    assert_eq!(moved, text.as_bytes(), "内容は 1 byte も不変");
    fs::remove_dir_all(&place.dir).ok();
}

/// (2) sid 違い: 現在 sid の名義へ移し、`consumed-from: <元 sid>` を frontmatter の末尾に 1 行だけ足す（本文は不変）。
#[test]
fn seat_wm_consume_moves_other_sid_to_current_name_with_consumed_from() {
    let place = wm_place();
    wm_stamp(&place, &["sid-new"]);
    let text = wm_body(WM_TARGET);
    let source = wm_raw(&place, "working-memory.sid-prev.md", &text);
    let out = wm_consume(&place);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        "seat: consumed file=working-memory.sid-new.consumed.md consumed-from=sid-prev\n",
        "stdout 1 行"
    );
    assert!(!source.exists(), "元 file は不在");
    assert_eq!(wm_names(&place), vec!["working-memory.sid-new.consumed.md".to_owned()], "現在 sid の名義 1 つだけ");
    let moved = fs::read_to_string(place.wm.join("working-memory.sid-new.consumed.md")).unwrap_or_default();
    let want = format!(
        "---\nschema: 1\nseat: {WM_TARGET}\ntrigger: manual\nconsumed-from: sid-prev\n---\n\n## 計画弧・次のステップ\n- 続き\n---\n末尾の行\n"
    );
    assert_eq!(moved, want, "frontmatter の末尾に 1 行・他は不変");
    assert_eq!(moved.matches("consumed-from:").count(), 1, "1 行だけ");
    assert_eq!(moved.len(), text.len() + "consumed-from: sid-prev\n".len(), "増えたのは 1 行分の byte だけ");
    fs::remove_dir_all(&place.dir).ok();
}

/// (3) 冪等: 2 回目は rc 0 `already` で file は不変。(4) 0 件かつ消費済みも無い → rc 1 `wm-missing`。
#[test]
fn seat_wm_consume_is_idempotent_and_refuses_when_nothing_to_consume() {
    let place = wm_place();
    wm_stamp(&place, &["sid-7"]);
    let missing = wm_consume(&place);
    assert_eq!(rc_of(&missing), i32::from(RC_REFUSED), "stdout={}", stdout_of(&missing));
    assert_eq!(stderr_of(&missing), "seat: consume refused reason=wm-missing\n", "理由");
    assert!(stdout_of(&missing).is_empty(), "断る周は stdout に書かない");
    assert!(wm_names(&place).is_empty(), "file を作らない");

    wm_raw(&place, "working-memory.sid-6.md", &wm_body(WM_TARGET));
    let first = wm_consume(&place);
    assert_eq!(rc_of(&first), i32::from(RC_OK), "stderr={}", stderr_of(&first));
    let consumed = place.wm.join("working-memory.sid-7.consumed.md");
    let before = fs::read(&consumed).unwrap_or_default();
    let mtime = mtime_of(&consumed);
    let second = wm_consume(&place);
    assert_eq!(rc_of(&second), i32::from(RC_OK), "2 回目も rc 0: {}", stderr_of(&second));
    assert_eq!(stdout_of(&second), "seat: consumed already file=working-memory.sid-7.consumed.md\n", "already");
    assert_eq!(fs::read(&consumed).unwrap_or_default(), before, "file は不変");
    assert_eq!(mtime_of(&consumed), mtime, "書き直さない");
    assert_eq!(wm_names(&place), vec!["working-memory.sid-7.consumed.md".to_owned()], "増えも減りもしない");
    fs::remove_dir_all(&place.dir).ok();
}

/// (5) 自席の未 consumed が 2 件 → rc 1 `wm-ambiguous n=2` で両 file 不変。(6) 他席の退避物・消費済み・退避物以外は不変。
#[test]
fn seat_wm_consume_refuses_ambiguous_and_leaves_other_seats_alone() {
    let place = wm_place();
    wm_stamp(&place, &["sid-9"]);
    let other = wm_raw(&place, "working-memory.sid-9x.md", &wm_body("other:1"));
    let decoy = wm_raw(&place, "notes-for-working-memory.md", &wm_body(WM_TARGET));
    let old = wm_raw(&place, "working-memory.sid-1.consumed.md", &wm_body(WM_TARGET));
    let fixed: Vec<(PathBuf, Vec<u8>)> =
        [&other, &decoy, &old].iter().map(|path| ((*path).clone(), fs::read(path).unwrap_or_default())).collect();

    let a = wm_raw(&place, "working-memory.sid-a.md", &wm_body(WM_TARGET));
    let b = wm_raw(&place, "working-memory.sid-9.md", &wm_body(WM_TARGET));
    let names = wm_names(&place);
    let out = wm_consume(&place);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "stdout={}", stdout_of(&out));
    assert_eq!(stderr_of(&out), "seat: consume refused reason=wm-ambiguous n=2\n", "理由と件数");
    assert!(stdout_of(&out).is_empty(), "断る周は stdout に書かない");
    assert_eq!(wm_names(&place), names, "何も動かさない");
    for path in [&a, &b] {
        assert_eq!(fs::read_to_string(path).unwrap_or_default(), wm_body(WM_TARGET), "両 file 不変: {}", path.display());
    }

    fs::remove_file(&a).ok();
    let one = wm_consume(&place);
    assert_eq!(rc_of(&one), i32::from(RC_OK), "自席 1 件なら消費する: {}", stderr_of(&one));
    assert_eq!(stdout_of(&one), "seat: consumed file=working-memory.sid-9.consumed.md\n", "自席だけを消費");
    for (path, before) in &fixed {
        assert_eq!(&fs::read(path).unwrap_or_default(), before, "他席・消費済み・退避物以外は不変: {}", path.display());
    }
    fs::remove_dir_all(&place.dir).ok();
}

/// (7) rename 先が既に在る → rc 1 `consumed-exists` で両 file 不変（上書きしない・N1）。同 sid・sid 違いの 2 経路。
#[test]
fn seat_wm_consume_refuses_when_destination_already_exists() {
    for source_name in ["working-memory.sid-c.md", "working-memory.sid-b.md"] {
        let place = wm_place();
        wm_stamp(&place, &["sid-c"]);
        let existing = wm_raw(&place, "working-memory.sid-c.consumed.md", "---\nseat: wm:1\n---\n既在の消費済み\n");
        let source = wm_raw(&place, source_name, &wm_body(WM_TARGET));
        let out = wm_consume(&place);
        assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{source_name}: stdout={}", stdout_of(&out));
        assert_eq!(stderr_of(&out), "seat: consume refused reason=consumed-exists\n", "{source_name}: 理由");
        assert_eq!(fs::read_to_string(&existing).unwrap_or_default(), "---\nseat: wm:1\n---\n既在の消費済み\n", "既在は不変");
        assert_eq!(fs::read_to_string(&source).unwrap_or_default(), wm_body(WM_TARGET), "{source_name}: 元も不変");
        fs::remove_dir_all(&place.dir).ok();
    }
}

/// (8) 打刻が無い → rc 1 `sid-missing`・sid が空 → rc 1 `sid-empty`。いずれも file を動かさない。
#[test]
fn seat_wm_consume_refuses_without_a_stamped_sid() {
    let place = wm_place();
    let source = wm_raw(&place, "working-memory.sid-d.md", &wm_body(WM_TARGET));
    let missing = wm_consume(&place);
    assert_eq!(rc_of(&missing), i32::from(RC_REFUSED), "stdout={}", stdout_of(&missing));
    assert_eq!(stderr_of(&missing), "seat: consume refused reason=sid-missing\n", "理由");
    assert!(source.exists(), "動かさない");

    wm_stamp(&place, &[""]);
    let empty = wm_consume(&place);
    assert_eq!(rc_of(&empty), i32::from(RC_REFUSED), "stdout={}", stdout_of(&empty));
    assert_eq!(stderr_of(&empty), "seat: consume refused reason=sid-empty\n", "理由");
    assert_eq!(wm_names(&place), vec!["working-memory.sid-d.md".to_owned()], "動かさない");
    fs::remove_dir_all(&place.dir).ok();
}

/// (9) 使い方の外形に consume が載る（snapshot と独立に字面で測る）・値欠けの flag は使い方で断る。
#[test]
fn seat_wm_consume_is_listed_in_usage_and_refuses_missing_flags() {
    let usage = stderr_of(&run_seat(&[]));
    assert!(usage.contains("|consume --target T --wm-dir DIR|"), "usage に consume（後ろに register が続く）: {usage}");
    let out = run_seat(&["consume", "--target", WM_TARGET]);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "--wm-dir 欠けは rc 1");
    assert_eq!(stderr_of(&out), usage, "使い方で断る");
}

// ─────────────────── 作業記憶の復元（rebrief・設計 working-memory.md §5.2 / §9 契約 (b)） ───────────────────

/// DATA を出せない周の rc（契約の字面から持つ）。
const RC_DATA_BROKEN: i32 = 2;

/// 偽の台帳（open 2・in_progress 1・blocked 1・`updated_at` の無い 1 件を含む）。
const BD_JSON: &str = concat!(
    "[{\"id\":\"s2-07l.61\",\"title\":\"裁定の記録\",\"status\":\"in_progress\",\"updated_at\":\"2026-09-12T02:01:00Z\",\"priority\":1},\n",
    " {\"id\":\"s2-1\",\"title\":\"a\",\"status\":\"open\",\"updated_at\":\"2026-09-11T00:00:00Z\"},\n",
    " {\"id\":\"s2-2\",\"title\":\"b\",\"status\":\"blocked\",\"updated_at\":\"2026-09-11T00:00:00Z\"},\n",
    " {\"id\":\"s2-3\",\"title\":\"c\",\"status\":\"open\"}]\n",
);

/// 復元の歯の節 1（従属行・連続空白・全角空白・閉じた状態の行を含む＝逐語でなければ字面が変わる）。
const REBRIEF_USER: &str = concat!(
    "- [2026-09-12 10:00] 「A を  そのまま、直せ。」 → 状態: 未着手\n",
    "  補足の従属行（逐語・全角　空白）\n",
    "- [2026-09-12 10:05] 「B は済んだ」 → 状態: 完了 s2-07l.1\n",
);

/// 復元の歯の節 3（Resolved / Unresolved / Unchecked / none × 2・`[hard候補]` は pointer 有りと無しの 2 行）。
const REBRIEF_DIRECTIVES: &str = concat!(
    "- [hard候補] [P0] since=2026-09-01 prose は規則でない → SSOT: 憲法 N2\n",
    "- [auto] [P1] since=2026-09-01 無い条 → SSOT: 憲法 C99\n",
    "- [confirm] [P1] since=2026-09-01 台帳の続き → SSOT: s2-07l.61\n",
    "- [hard候補] [P1] since=2026-09-01 矢印の無い命令 s2-07l.140 を見る\n",
    "- [auto] [P2] since=2026-09-01 裁定だけ → SSOT: user 裁定 2026-09-12T02:01Z\n",
);

/// found の周の DATA 全体（契約の字面から組む）。
const REBRIEF_FOUND: &str = concat!(
    "[SID] sid-now\n",
    "[WM] found file=working-memory.sid-now.md\n",
    "[PLUGIN] drift=unrecorded\n",
    "[WM-PLAN] - 次は rebrief の land\n",
    "[WM-USER-DIRECTIVE] - [2026-09-12 10:00] 「A を  そのまま、直せ。」 → 状態: 未着手\n",
    "[WM-USER-DIRECTIVE]   補足の従属行（逐語・全角　空白）\n",
    "[WM-USER-DIRECTIVE] - [2026-09-12 10:05] 「B は済んだ」 → 状態: 完了 s2-07l.1\n",
    "[WM-DIRECTIVE] kind=Constitution resolution=Resolved line=- [hard候補] [P0] since=2026-09-01 prose は規則でない → SSOT: 憲法 N2\n",
    "[WM-DIRECTIVE] kind=Constitution resolution=Unresolved line=- [auto] [P1] since=2026-09-01 無い条 → SSOT: 憲法 C99\n",
    "[WM-DIRECTIVE] kind=Ledger resolution=Unchecked line=- [confirm] [P1] since=2026-09-01 台帳の続き → SSOT: s2-07l.61\n",
    "[WM-DIRECTIVE] kind=none resolution=none line=- [hard候補] [P1] since=2026-09-01 矢印の無い命令 s2-07l.140 を見る\n",
    "[WM-DIRECTIVE] kind=none resolution=none line=- [auto] [P2] since=2026-09-01 裁定だけ → SSOT: user 裁定 2026-09-12T02:01Z\n",
    "[WM-DIRECTIVE-COUNT] total=5 provisional=2 unresolved=1\n",
    "[MAIN] sha=unknown origin=unknown porcelain=unknown reason=head-unreadable\n",
    "[RUN-COUNT] n=0\n",
    "[RUN-NONE]\n",
    "[SEAT-COUNT] n=0\n",
    "[SEAT-NONE]\n",
    "[WIN-UNKNOWN] reason=consumed-unreadable\n",
    "[ORPHAN-WM] file=working-memory.sid-other.md seat=other:1\n",
    "[BD-COUNT] open=2 in_progress=1 blocked=1\n",
    "[MEMO-DUE-COUNT] n=0 of=0\n",
    "[MEMO-DUE-NONE]\n",
    "[MEMO-STALE-COUNT] n=0 of=0 unreadable=0\n",
    "[MEMO-STALE-NONE]\n",
    "[BD-INPROGRESS] s2-07l.61 updated=2026-09-12T02:01:00Z 裁定の記録\n",
    "[DIFF] s2-07l.61 bd=in_progress\n",
    "[DIFF] s2-07l.140 bd=unknown\n",
    "[TICKET-CANDIDATE] - [hard候補] [P1] since=2026-09-01 矢印の無い命令 s2-07l.140 を見る\n",
);

/// 断りの 1 行（契約の字面から組む）。
fn rebrief_refusal(reason: &str) -> String {
    format!("seat: rebrief unavailable reason={reason}\n")
}

/// 偽の bd を 1 本作る（headless の fake_claude と同じ型）。引数を `<name>.args` へ写し、`body` を stdout へ
/// 出して `rc` で終わる。`sleep_s` > 0 なら出力の前に `exec sleep` で眠る（殺せば 1 process で終わる）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn fake_bd(dir: &Path, name: &str, body: &str, rc: u8, sleep_s: u64) -> String {
    let d = dir.display().to_string();
    fs::write(dir.join(format!("{name}.json")), body).expect("body を書ける");
    let sleep = if sleep_s > 0 { format!("exec sleep {sleep_s}\n") } else { String::new() };
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{d}/{name}.args\"\n{sleep}cat \"{d}/{name}.json\"\nexit {rc}\n"
    );
    let path = dir.join(name);
    fs::write(&path, script).expect("fake を書ける");
    let mut perm = fs::metadata(&path).expect("fake の権限を読める").permissions();
    perm.set_mode(0o755);
    fs::set_permissions(&path, perm).expect("fake を実行可能にできる");
    path.display().to_string()
}

/// argv で body を選ぶ偽 bd を 1 本作る（上の [`fake_bd`] と同じ script の型・rc は 0）。argv に `--all` を含む周は
/// `all`（closed を含む一覧）を、含まない周は `open`（台帳の既定＝closed を含まない）を stdout へ出す。argv の写しは
/// 既存と同じ `<name>.args`（設計 working-memory.md §14・`s2-07l.406`）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn fake_bd_by_args(dir: &Path, name: &str, open: &str, all: &str) -> String {
    let d = dir.display().to_string();
    fs::write(dir.join(format!("{name}.open.json")), open).expect("open の body を書ける");
    fs::write(dir.join(format!("{name}.all.json")), all).expect("all の body を書ける");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{d}/{name}.args\"\ncase \"$*\" in\n\
         *--all*) cat \"{d}/{name}.all.json\" ;;\n\
         *) cat \"{d}/{name}.open.json\" ;;\nesac\nexit 0\n"
    );
    let path = dir.join(name);
    fs::write(&path, script).expect("fake を書ける");
    let mut perm = fs::metadata(&path).expect("fake の権限を読める").permissions();
    perm.set_mode(0o755);
    fs::set_permissions(&path, perm).expect("fake を実行可能にできる");
    path.display().to_string()
}

/// 台帳の待ち上限と memo の閾値 2 つ（3 日 / P2）を持つ rules の fixture を書き、`--rules` に渡す path を返す。
fn rebrief_rules(place: &WmPlace, secs: u64) -> String {
    fixture(
        &place.dir,
        "rebrief-rules.toml",
        &format!(
            "schema = 1\n\n[[rule]]\nid = \"seat.ledger_timeout_s\"\nkind = \"LedgerTimeoutS\"\nvalue = {secs}\n\
             enabled = true\nruling = \"user 2026-09-12T02:01Z\"\nruled_at = \"2026-09-12\"\n{MEMO_RULES}"
        ),
    )
}

/// memo の閾値 2 行（裁定 id `user 2026-09-13T14:06Z` の値）。
const MEMO_RULES: &str = concat!(
    "\n[[rule]]\nid = \"ledger.memo_stale_days\"\nkind = \"MemoStaleDays\"\nvalue = 3\n",
    "enabled = true\nruling = \"user 2026-09-13T14:06Z\"\nruled_at = \"2026-09-13\"\n",
    "\n[[rule]]\nid = \"ledger.memo_stale_priority\"\nkind = \"MemoStalePriority\"\nvalue = 2\n",
    "enabled = true\nruling = \"user 2026-09-13T14:06Z\"\nruled_at = \"2026-09-13\"\n",
);

/// `seat rebrief` を 1 回撃つ（`extra` は追加の flag）。
fn wm_rebrief(place: &WmPlace, bd: &str, extra: &[&str]) -> Output {
    let (wm, state, anchor) = (
        place.wm.display().to_string(),
        place.state.display().to_string(),
        place.anchor.display().to_string(),
    );
    let mut args = vec![
        "rebrief", "--target", WM_TARGET, "--wm-dir", &wm, "--state-dir", &state, "--anchor", &anchor, "--bd", bd,
    ];
    args.extend_from_slice(extra);
    run_seat(&args)
}

/// 3 節を持つ退避物の全文。
fn rebrief_body(seat: &str) -> String {
    format!(
        "---\nschema: 1\nseat: {seat}\ntrigger: manual\n---\n\n{WM_HEAD_USER}\n{REBRIEF_USER}\n{WM_HEAD_PLAN}\n- 次は rebrief の land\n\n{WM_HEAD_DIRECTIVES}\n{REBRIEF_DIRECTIVES}"
    )
}

/// `schema` 無し・節 2 だけの退避物（前の版の skill が書いた形）。
fn rebrief_legacy_body() -> String {
    format!("---\nseat: {WM_TARGET}\n---\n\n{WM_HEAD_PLAN}\n- 続き\n")
}

/// found の場所（自席 1・別席 1・消費済み 1〔数えない〕・偽 bd）。
fn rebrief_found() -> (WmPlace, String) {
    let place = wm_place();
    wm_stamp(&place, &["sid-old", "sid-now"]);
    wm_raw(&place, "working-memory.sid-now.md", &rebrief_body(WM_TARGET));
    wm_raw(&place, "working-memory.sid-other.md", &wm_body("other:1"));
    wm_raw(&place, "working-memory.sid-0.consumed.md", &rebrief_body(WM_TARGET));
    let bd = fake_bd(&place.dir, "bd", BD_JSON, 0, 0);
    (place, bd)
}

/// 外形 snapshot の 3 形（found / candidate / missing）を撃った出力。
pub(super) fn rebrief_forms() -> Vec<Output> {
    let (found, bd) = rebrief_found();
    let mut outs = vec![wm_rebrief(&found, &bd, &[])];
    fs::remove_dir_all(&found.dir).ok();
    for own in [Some("working-memory.sid-prev.md"), None] {
        let place = wm_place();
        wm_stamp(&place, &["sid-new"]);
        if let Some(name) = own {
            wm_raw(&place, name, &rebrief_legacy_body());
        }
        let empty = fake_bd(&place.dir, "bd", "[]", 0, 0);
        outs.push(wm_rebrief(&place, &empty, &[]));
        fs::remove_dir_all(&place.dir).ok();
    }
    outs
}

/// (1) found の周は全段を marker の宣言順に出す（stdout 全体を契約の字面で照合・記録の無い席の `[PLUGIN]` は
/// `drift=unrecorded` の 1 行・`s2-07l.304`・現在地の段は anchor が repo でなく log も消費済みの退避時刻も無い形・
/// `s2-07l.326`）・bd は `--readonly` で撃つ。marker の母集団は 36（`.304` で `[PLUGIN]` +1・`.326` で現在地 +11）。
#[test]
fn seat_wm_rebrief_lists_found_wm_with_every_stage_in_marker_order() {
    let (place, bd) = rebrief_found();
    let out = wm_rebrief(&place, &bd, &[]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert!(stderr_of(&out).is_empty(), "stderr は空: {}", stderr_of(&out));
    assert_eq!(stdout_of(&out), REBRIEF_FOUND, "DATA 全体");
    assert_eq!(vessel::seat::rebrief::ALL.len(), 36, "marker の母集団（`[PLUGIN]` で 24 → 25・現在地の 11 で 36）");
    assert_eq!(
        fs::read_to_string(place.dir.join("bd.args")).unwrap_or_default(),
        "--readonly\nlist\n--all\n--limit\n0\n--json\n",
        "台帳は --readonly の子 process で読む（`--all` で closed も母集団に入れる・`s2-07l.406`）"
    );
    fs::remove_dir_all(&place.dir).ok();
}

/// 席の打刻 dir に読み込み元の記録（`.303` の `plugin` 1 行・`hooks=` は root の hooks.json の digest・`binary=` は `binary`）を
/// 書く。root は `<place.dir>/plugin-root`（`hooks/hooks.json` = `body`・無ければ file を置かない）。
fn rebrief_plugin_record(place: &WmPlace, body: Option<&str>, binary: &str) -> PathBuf {
    use vessel::hook::vessel::digest;
    let root = place.dir.join("plugin-root");
    fs::create_dir_all(root.join("hooks")).ok();
    match body {
        Some(text) => fs::write(digest::hooks_path(&root), text).ok(),
        None => fs::remove_file(digest::hooks_path(&root)).ok(),
    };
    let written = digest::write(&seat_dir_of(&place.state, WM_SEAT_DIR), &root, "sid-now", binary);
    assert_eq!(written, Ok(()), "記録を書ける");
    root
}

/// (1′) DATA の `[PLUGIN]` は `[WM]` の**直後**に 1 行（consumer-sync.md §6・`s2-07l.304`）: 記録が在れば `root=<root>
/// hooks=<記録の digest> binary=<記録の sha> drift=<語>`——root の hooks.json を記録の後に変えた席は `drift=hooks`・binary
/// だけ違えば `drift=binary`・両方は `hooks+binary`・どちらも同じは `none`・hooks.json を外せば `unreadable`。記録を外した周は
/// `[PLUGIN] drift=unrecorded`（「無い」を黙らせない・rc 0）。base に marker が無い（RED）。
#[test]
fn seat_wm_rebrief_carries_the_plugin_line_in_marker_order() {
    use vessel::hook::vessel::digest;
    let (place, bd) = rebrief_found();
    let built = env!("SCRIBE2_BUILD_COMMIT");
    let (body_a, body_b) = ("{\"hooks\":{}}\n", "{\"hooks\":{\"Stop\":[]}}\n");
    let digest_a = digest::fnv1a_64(body_a.as_bytes());
    let root = rebrief_plugin_record(&place, Some(body_a), built);
    fs::write(digest::hooks_path(&root), body_b).ok();
    let root_s = root.display().to_string();
    let plugin_line_of = |case: &str| -> String {
        let out = wm_rebrief(&place, &bd, &[]);
        assert_eq!(rc_of(&out), i32::from(RC_OK), "{case}: stderr={}", stderr_of(&out));
        let text = stdout_of(&out);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.get(1).copied(), Some("[WM] found file=working-memory.sid-now.md"), "{case}: {text}");
        assert_eq!(lines.iter().filter(|line| line.starts_with("[PLUGIN]")).count(), 1, "{case}: 1 行だけ: {text}");
        lines.get(2).map(|line| (*line).to_owned()).unwrap_or_default()
    };
    assert_eq!(
        plugin_line_of("hooks"),
        format!("[PLUGIN] root={root_s} hooks={digest_a} binary={built} drift=hooks"),
        "記録の digest と今の digest が違う"
    );
    rebrief_plugin_record(&place, Some(body_a), "000000000000");
    assert_eq!(
        plugin_line_of("binary"),
        format!("[PLUGIN] root={root_s} hooks={digest_a} binary=000000000000 drift=binary"),
        "binary だけの食い違い"
    );
    fs::write(digest::hooks_path(&root), body_b).ok();
    assert!(plugin_line_of("hooks+binary").ends_with(" binary=000000000000 drift=hooks+binary"), "両方");
    rebrief_plugin_record(&place, Some(body_a), built);
    assert_eq!(plugin_line_of("none"), format!("[PLUGIN] root={root_s} hooks={digest_a} binary={built} drift=none"));
    fs::remove_file(digest::hooks_path(&root)).ok();
    assert!(plugin_line_of("unreadable").ends_with(&format!(" hooks={digest_a} binary={built} drift=unreadable")), "hooks.json が無い");
    fs::remove_file(digest::record_path(&seat_dir_of(&place.state, WM_SEAT_DIR))).ok();
    assert_eq!(plugin_line_of("unrecorded"), "[PLUGIN] drift=unrecorded", "記録が無い周は 1 語");
    fs::remove_dir_all(&place.dir).ok();
}

/// (2) sid 違いの自席退避物は candidate・他席のは `[ORPHAN-WM]`（file は不変）・0 件は missing・2 件は ambiguous。
#[test]
fn seat_wm_rebrief_marks_other_sid_as_candidate_and_other_seat_as_orphan() {
    let place = wm_place();
    wm_stamp(&place, &["sid-new"]);
    let bd = fake_bd(&place.dir, "bd", "[]", 0, 0);
    let own = wm_raw(&place, "working-memory.sid-prev.md", &rebrief_legacy_body());
    let other = wm_raw(&place, "working-memory.sid-x.md", &wm_body("other:1"));
    let out = wm_rebrief(&place, &bd, &[]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    let text = stdout_of(&out);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.get(1).copied(), Some("[WM] candidate file=working-memory.sid-prev.md sid=sid-prev"), "{text}");
    assert!(lines.contains(&"[ORPHAN-WM] file=working-memory.sid-x.md seat=other:1"), "{text}");
    assert!(!lines.contains(&"[ORPHAN-NONE]"), "orphan が在る周に空印を出さない: {text}");
    assert_eq!(fs::read_to_string(&other).unwrap_or_default(), wm_body("other:1"), "他席は不変");
    assert!(own.exists(), "自席も消費しない");

    let second = wm_raw(&place, "working-memory.sid-new.md", &rebrief_legacy_body());
    let ambiguous = stdout_of(&wm_rebrief(&place, &bd, &[]));
    assert!(ambiguous.contains("\n[WM] ambiguous n=2\n") && !ambiguous.contains("[WM-PLAN"), "採用しない: {ambiguous}");

    fs::remove_file(&own).ok();
    fs::remove_file(&second).ok();
    let missing = wm_rebrief(&place, &bd, &[]);
    assert_eq!(rc_of(&missing), i32::from(RC_OK), "missing は正常: {}", stderr_of(&missing));
    assert_eq!(
        stdout_of(&missing),
        concat!(
            "[SID] sid-new\n",
            "[WM] missing\n",
            "[PLUGIN] drift=unrecorded\n",
            "[MAIN] sha=unknown origin=unknown porcelain=unknown reason=head-unreadable\n",
            "[RUN-COUNT] n=0\n",
            "[RUN-NONE]\n",
            "[SEAT-COUNT] n=0\n",
            "[SEAT-NONE]\n",
            "[WIN-UNKNOWN] reason=no-consumed\n",
            "[ORPHAN-WM] file=working-memory.sid-x.md seat=other:1\n",
            "[BD-COUNT] open=0 in_progress=0 blocked=0\n",
            "[MEMO-DUE-COUNT] n=0 of=0\n",
            "[MEMO-DUE-NONE]\n",
            "[MEMO-STALE-COUNT] n=0 of=0 unreadable=0\n",
            "[MEMO-STALE-NONE]\n",
            "[BD-INPROGRESS-NONE]\n",
        )
    );
    fs::remove_dir_all(&place.dir).ok();
}

/// (3) 節 3 の 5 行が kind・resolution 付きで列挙され、COUNT が列挙から数えた値と一致する。
#[test]
fn seat_wm_rebrief_directive_count_matches_the_listing() {
    let (place, bd) = rebrief_found();
    let text = stdout_of(&wm_rebrief(&place, &bd, &[]));
    let listed: Vec<&str> = text.lines().filter(|line| line.starts_with("[WM-DIRECTIVE] ")).collect();
    let pairs: Vec<(&str, &str)> = listed
        .iter()
        .filter_map(|line| {
            let rest = line.strip_prefix("[WM-DIRECTIVE] kind=")?;
            let (kind, rest) = rest.split_once(" resolution=")?;
            Some((kind, rest.split_once(" line=")?.0))
        })
        .collect();
    assert_eq!(
        pairs,
        [("Constitution", "Resolved"), ("Constitution", "Unresolved"), ("Ledger", "Unchecked"), ("none", "none"), ("none", "none")],
        "{text}"
    );
    let provisional = pairs.iter().filter(|(kind, _)| *kind == "none").count();
    let unresolved = pairs.iter().filter(|(_, resolution)| *resolution == "Unresolved").count();
    let want = format!("[WM-DIRECTIVE-COUNT] total={} provisional={provisional} unresolved={unresolved}", listed.len());
    assert_eq!(want, "[WM-DIRECTIVE-COUNT] total=5 provisional=2 unresolved=1", "母集団 5 行");
    assert_eq!(text.lines().filter(|line| line.starts_with("[WM-DIRECTIVE-COUNT]")).collect::<Vec<_>>(), [want.as_str()], "{text}");
    fs::remove_dir_all(&place.dir).ok();
}

/// (4) 節 1 の各行は 1 byte も変えずに出る（従属行の字下げ・連続空白・全角空白・閉じた状態の行も）。
#[test]
fn seat_wm_rebrief_user_directives_are_verbatim() {
    let (place, bd) = rebrief_found();
    let text = stdout_of(&wm_rebrief(&place, &bd, &[]));
    let listed: Vec<&str> = text.lines().filter_map(|line| line.strip_prefix("[WM-USER-DIRECTIVE] ")).collect();
    let source: Vec<&str> = REBRIEF_USER.lines().collect();
    assert_eq!(source.len(), 3, "母集団 3 行");
    assert_eq!(listed, source, "逐語: {text}");
    fs::remove_dir_all(&place.dir).ok();
}

/// (5) 偽 bd の fixture で BD-COUNT / INPROGRESS / DIFF（言及 id の status・台帳に無い id は unknown）。
/// 節 1 の id は DIFF に載らない・空の台帳は正当な 0。
#[test]
fn seat_wm_rebrief_reports_ledger_counts_and_diff_of_mentioned_ids() {
    let (place, bd) = rebrief_found();
    let text = stdout_of(&wm_rebrief(&place, &bd, &[]));
    let pick = |head: &str| -> Vec<String> {
        text.lines().filter(|line| line.starts_with(head)).map(str::to_owned).collect()
    };
    assert_eq!(pick("[BD-COUNT]"), ["[BD-COUNT] open=2 in_progress=1 blocked=1"]);
    assert_eq!(pick("[BD-INPROGRESS"), ["[BD-INPROGRESS] s2-07l.61 updated=2026-09-12T02:01:00Z 裁定の記録"]);
    assert_eq!(pick("[DIFF"), ["[DIFF] s2-07l.61 bd=in_progress", "[DIFF] s2-07l.140 bd=unknown"]);
    assert!(!text.contains("s2-07l.1 bd="), "節 1 の id は比べない: {text}");

    let empty = fake_bd(&place.dir, "bd-empty", "[]", 0, 0);
    let zero = stdout_of(&wm_rebrief(&place, &empty, &[]));
    assert!(zero.contains("\n[BD-COUNT] open=0 in_progress=0 blocked=0\n"), "{zero}");
    assert!(zero.contains("\n[MEMO-STALE-NONE]\n[BD-INPROGRESS-NONE]\n"), "{zero}");
    assert!(zero.contains("\n[DIFF] s2-07l.61 bd=unknown\n[DIFF] s2-07l.140 bd=unknown\n"), "{zero}");
    fs::remove_dir_all(&place.dir).ok();
}

/// (6) 偽 bd が rc 1・JSON ゴミ・timeout（stub が眠る・rules の fixture で 1 s）・不在のそれぞれで DATA 0 行 + rc 2。
#[test]
fn seat_wm_rebrief_emits_no_data_when_ledger_is_unreadable() {
    let (place, _) = rebrief_found();
    let rules = rebrief_rules(&place, 1);
    let cases = [
        ("rc 1", fake_bd(&place.dir, "bd-rc1", BD_JSON, 1, 0)),
        ("JSON ゴミ", fake_bd(&place.dir, "bd-junk", "[{\"id\": \"s2-1\", ", 0, 0)),
        ("形違い", fake_bd(&place.dir, "bd-shape", "{\"id\":\"s2-1\",\"status\":\"open\"}", 0, 0)),
        ("timeout", fake_bd(&place.dir, "bd-slow", BD_JSON, 0, 5)),
        ("不在", place.dir.join("no-such-bd").display().to_string()),
    ];
    for (label, bd) in cases {
        let started = Instant::now();
        let out = wm_rebrief(&place, &bd, &["--rules", &rules]);
        assert_eq!(rc_of(&out), RC_DATA_BROKEN, "{label}: stdout={}", stdout_of(&out));
        assert!(stdout_of(&out).is_empty(), "{label}: DATA は 0 行: {}", stdout_of(&out));
        assert_eq!(stderr_of(&out), rebrief_refusal("ledger-unreadable"), "{label}: 理由");
        assert!(started.elapsed() < Duration::from_secs(4), "{label}: 待ち上限で打ち切る: {:?}", started.elapsed());
    }
    let fast = fake_bd(&place.dir, "bd-fast", BD_JSON, 0, 0);
    assert_eq!(rc_of(&wm_rebrief(&place, &fast, &["--rules", &rules])), i32::from(RC_OK), "同じ rules で読める bd は通る");
    fs::remove_dir_all(&place.dir).ok();
}

/// (7) wm dir が読めない（不在・file）周は rc 2 `wm-dir-unreadable`・anchor が dir でない周は `anchor-missing`。
#[test]
fn seat_wm_rebrief_refuses_when_wm_dir_or_anchor_is_unreadable() {
    let (place, bd) = rebrief_found();
    let (state, anchor) = (place.state.display().to_string(), place.anchor.display().to_string());
    let absent = place.dir.join("no-such-wm").display().to_string();
    let file = fixture(&place.dir, "not-a-dir", "x\n");
    for wm in [absent, file] {
        let out = run_seat(&[
            "rebrief", "--target", WM_TARGET, "--wm-dir", &wm, "--state-dir", &state, "--anchor", &anchor, "--bd", &bd,
        ]);
        assert_eq!(rc_of(&out), RC_DATA_BROKEN, "{wm}: stdout={}", stdout_of(&out));
        assert!(stdout_of(&out).is_empty(), "DATA は 0 行");
        assert_eq!(stderr_of(&out), rebrief_refusal("wm-dir-unreadable"), "{wm}");
    }
    let no_anchor = place.dir.join("no-such-anchor").display().to_string();
    let wm = place.wm.display().to_string();
    let out = run_seat(&[
        "rebrief", "--target", WM_TARGET, "--wm-dir", &wm, "--state-dir", &state, "--anchor", &no_anchor, "--bd", &bd,
    ]);
    assert_eq!((rc_of(&out), stderr_of(&out)), (RC_DATA_BROKEN, rebrief_refusal("anchor-missing")));
    assert!(stdout_of(&out).is_empty(), "DATA は 0 行");
    fs::remove_dir_all(&place.dir).ok();
}

/// (8) read-only: 実行前後で wm dir・state dir・anchor の全 entry の size / mtime が同一・entry も増えない（lock 不在）。
#[test]
fn seat_wm_rebrief_changes_nothing_on_disk() {
    let (place, bd) = rebrief_found();
    for root in [&place.wm, &place.state, &place.anchor] {
        for (path, _, _) in tree_stat(root) {
            if path.is_file() {
                backdate(&path, 600);
            }
        }
    }
    let before: Vec<_> = [&place.wm, &place.state, &place.anchor].iter().map(|root| tree_stat(root)).collect();
    assert!(before.iter().map(Vec::len).sum::<usize>() >= 8, "母集団: {before:?}");
    for _ in 0..2 {
        let out = wm_rebrief(&place, &bd, &[]);
        assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    }
    let after: Vec<_> = [&place.wm, &place.state, &place.anchor].iter().map(|root| tree_stat(root)).collect();
    assert_eq!(after, before, "何も書かない・何も作らない");
    fs::remove_dir_all(&place.dir).ok();
}

/// (9) `[TICKET-CANDIDATE]` は `[hard候補]` かつ pointer なしの行だけ（pointer 有りの `[hard候補]`・pointer なしの `[auto]` は載らない）。
#[test]
fn seat_wm_rebrief_ticket_candidates_are_hard_candidates_without_pointer() {
    let (place, bd) = rebrief_found();
    let text = stdout_of(&wm_rebrief(&place, &bd, &[]));
    let candidates: Vec<&str> = text.lines().filter(|line| line.starts_with("[TICKET-CANDIDATE")).collect();
    assert_eq!(
        candidates,
        ["[TICKET-CANDIDATE] - [hard候補] [P1] since=2026-09-01 矢印の無い命令 s2-07l.140 を見る"],
        "{text}"
    );
    fs::remove_dir_all(&place.dir).ok();
}

/// (11) `schema` 無しの退避物も found として読める・空の節は空印で出す（「なし」に化けさせない）。
#[test]
fn seat_wm_rebrief_reads_schemaless_wm_and_marks_empty_sections() {
    let place = wm_place();
    wm_stamp(&place, &["sid-l"]);
    wm_raw(&place, "working-memory.sid-l.md", &rebrief_legacy_body());
    let bd = fake_bd(&place.dir, "bd", BD_JSON, 0, 0);
    let out = wm_rebrief(&place, &bd, &[]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        concat!(
            "[SID] sid-l\n",
            "[WM] found file=working-memory.sid-l.md\n",
            "[PLUGIN] drift=unrecorded\n",
            "[WM-PLAN] - 続き\n",
            "[WM-USER-DIRECTIVE-EMPTY]\n",
            "[WM-DIRECTIVE-EMPTY]\n",
            "[WM-DIRECTIVE-COUNT] total=0 provisional=0 unresolved=0\n",
            "[MAIN] sha=unknown origin=unknown porcelain=unknown reason=head-unreadable\n",
            "[RUN-COUNT] n=0\n",
            "[RUN-NONE]\n",
            "[SEAT-COUNT] n=0\n",
            "[SEAT-NONE]\n",
            "[WIN-UNKNOWN] reason=no-consumed\n",
            "[ORPHAN-NONE]\n",
            "[BD-COUNT] open=2 in_progress=1 blocked=1\n",
            "[MEMO-DUE-COUNT] n=0 of=0\n",
            "[MEMO-DUE-NONE]\n",
            "[MEMO-STALE-COUNT] n=0 of=0 unreadable=0\n",
            "[MEMO-STALE-NONE]\n",
            "[BD-INPROGRESS] s2-07l.61 updated=2026-09-12T02:01:00Z 裁定の記録\n",
            "[DIFF-NONE]\n",
            "[TICKET-CANDIDATE-NONE]\n",
        )
    );
    fs::remove_dir_all(&place.dir).ok();
}

/// (12) 打刻の sid が空なら rc 2 `sid-empty`・打刻が無ければ `sid-missing`（いずれも DATA 0 行）。
#[test]
fn seat_wm_rebrief_refuses_without_a_stamped_sid() {
    let (place, bd) = rebrief_found();
    wm_stamp(&place, &["sid-now", ""]);
    let empty = wm_rebrief(&place, &bd, &[]);
    assert_eq!(rc_of(&empty), RC_DATA_BROKEN, "stdout={}", stdout_of(&empty));
    assert!(stdout_of(&empty).is_empty(), "DATA は 0 行");
    assert_eq!(stderr_of(&empty), rebrief_refusal("sid-empty"));
    fs::remove_dir_all(&place.state).ok();
    let missing = wm_rebrief(&place, &bd, &[]);
    assert_eq!((rc_of(&missing), stderr_of(&missing)), (RC_DATA_BROKEN, rebrief_refusal("sid-missing")));
    assert!(stdout_of(&missing).is_empty(), "DATA は 0 行");
    fs::remove_dir_all(&place.dir).ok();
}

/// 使い方の外形に rebrief が載る（snapshot と独立に字面で測る）・必須 flag の欠けは rc 1 で使い方を返す。
#[test]
fn seat_wm_rebrief_is_listed_in_usage_and_refuses_missing_flags() {
    let usage = stderr_of(&run_seat(&[]));
    assert!(usage.contains("|rebrief --target T --wm-dir DIR --anchor DIR [--bd PATH]"), "usage に rebrief: {usage}");
    let out = run_seat(&["rebrief", "--target", WM_TARGET, "--wm-dir", "/nonexistent"]);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "--anchor 欠けは rc 1");
    assert_eq!(stderr_of(&out), usage, "使い方で断る");
    assert!(stdout_of(&out).is_empty(), "stdout は空");
}

// ─────── `[DIFF]` の母集団に閉じた bead を含める（設計 working-memory.md §14・`s2-07l.406`） ───────

/// `_diff_` の歯の節 3（閉じた id・開いた id・台帳に無い id を出現順に 1 行ずつ言及する）。
const REBRIEF_DIFF_DIRECTIVES: &str = concat!(
    "- [confirm] [P1] since=2026-09-01 閉じた続き → SSOT: s2-900\n",
    "- [confirm] [P1] since=2026-09-01 開いた続き → SSOT: s2-901\n",
    "- [confirm] [P1] since=2026-09-01 台帳に無い続き → SSOT: s2-902\n",
);

/// 台帳の 1 要素（status を明示・`labels` は JSON の字面）。
fn diff_issue(id: &str, status: &str, labels: &str, priority: u64, updated: &str) -> String {
    format!(
        "{{\"id\":\"{id}\",\"title\":\"t\",\"status\":\"{status}\",\"priority\":{priority},\"labels\":{labels},\"updated_at\":\"{updated}\"}}"
    )
}

/// 要素の列を `bd list --json` の配列にする。
fn diff_json(issues: &[String]) -> String {
    format!("[{}]\n", issues.join(",\n"))
}

/// `_diff_` の歯の場所（自席の退避物 1・打刻 1・argv で body を選ぶ偽 bd）。節 3 は [`REBRIEF_DIFF_DIRECTIVES`]。
fn rebrief_diff_place(open: &str, all: &str) -> (WmPlace, String) {
    let place = wm_place();
    wm_stamp(&place, &["sid-diff"]);
    let body = format!(
        "---\nschema: 1\nseat: {WM_TARGET}\ntrigger: manual\n---\n\n{WM_HEAD_PLAN}\n- 続き\n\n{WM_HEAD_DIRECTIVES}\n{REBRIEF_DIFF_DIRECTIVES}"
    );
    wm_raw(&place, "working-memory.sid-diff.md", &body);
    let bd = fake_bd_by_args(&place.dir, "bd-diff", open, all);
    (place, bd)
}

/// stdout の `[DIFF` 行。
fn diff_rows(text: &str) -> Vec<String> {
    text.lines().filter(|line| line.starts_with("[DIFF")).map(str::to_owned).collect()
}

/// (a) 閉じた bead は `bd=closed` と名乗り、`unknown` は**台帳に無い** id だけになる（設計 §14 の形 (1)(2)）。
/// 同じ退避物でも closed を含まない一覧（台帳の既定＝`--all` を無視する偽 bd）で撃つと、閉じた id が `unknown` に
/// 化ける——「無い」と「閉じた」を同じ語で名乗らないことを、母集団の違いそのもので測る。
#[test]
fn seat_wm_rebrief_diff_names_closed_beads_as_closed() {
    let updated = "2026-09-12T02:01:00Z";
    let open_only = diff_json(&[diff_issue("s2-901", "open", "[]", 1, updated)]);
    let with_closed = diff_json(&[
        diff_issue("s2-900", "closed", "[]", 1, updated),
        diff_issue("s2-901", "open", "[]", 1, updated),
    ]);
    let (place, bd) = rebrief_diff_place(&open_only, &with_closed);
    let out = wm_rebrief(&place, &bd, &[]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    let text = stdout_of(&out);
    assert_eq!(
        diff_rows(&text),
        ["[DIFF] s2-900 bd=closed", "[DIFF] s2-901 bd=open", "[DIFF] s2-902 bd=unknown"],
        "{text}"
    );

    let closed_blind = fake_bd(&place.dir, "bd-open-only", &open_only, 0, 0);
    let blind = stdout_of(&wm_rebrief(&place, &closed_blind, &[]));
    assert_eq!(
        diff_rows(&blind),
        ["[DIFF] s2-900 bd=unknown", "[DIFF] s2-901 bd=open", "[DIFF] s2-902 bd=unknown"],
        "closed を含まない母集団では閉じた id が「無い」に化ける: {blind}"
    );
    fs::remove_dir_all(&place.dir).ok();
}

/// (b) 台帳は 1 回だけ撃ち、argv の写しは `--readonly list --all --limit 0 --json`（`--all` は 1 回だけ・
/// 従来の 5 語は順も字面もそのまま）。
#[test]
fn seat_wm_rebrief_diff_passes_all_once() {
    let body = diff_json(&[diff_issue("s2-901", "open", "[]", 1, "2026-09-12T02:01:00Z")]);
    let (place, bd) = rebrief_diff_place(&body, &body);
    let out = wm_rebrief(&place, &bd, &[]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    let args = fs::read_to_string(place.dir.join("bd-diff.args")).unwrap_or_default();
    assert_eq!(
        args.lines().collect::<Vec<&str>>(),
        ["--readonly", "list", "--all", "--limit", "0", "--json"],
        "argv 全体"
    );
    assert_eq!(args.lines().filter(|word| *word == "--all").count(), 1, "--all は 1 回だけ: {args}");
    fs::remove_dir_all(&place.dir).ok();
}

/// (c) closed を含む母集団にしても、status で絞る既存の読み手は件数が変わらない: `[BD-COUNT]` の 3 値と
/// `[MEMO-*]` の行は closed を含まない一覧で撃った周と 1 byte 同じ（`[DIFF]` だけが変わる）。
#[test]
fn seat_wm_rebrief_diff_keeps_open_counts_unchanged() {
    let (four, ten) = (days_ago(4), days_ago(10));
    let open_rows = [
        diff_issue("s2-910", "open", MEMO_LABEL, 2, &four),
        diff_issue("s2-911", "open", "[]", 3, &four),
        diff_issue("s2-912", "in_progress", "[]", 1, &four),
        diff_issue("s2-913", "blocked", "[]", 1, &four),
        diff_issue("s2-901", "open", "[]", 1, &four),
    ];
    let mut all_rows = open_rows.to_vec();
    all_rows.push(diff_issue("s2-900", "closed", MEMO_LABEL, 1, &ten));
    all_rows.push(diff_issue("s2-914", "closed", "[]", 0, &ten));
    let (place, bd) = rebrief_diff_place(&diff_json(&open_rows), &diff_json(&all_rows));
    let out = wm_rebrief(&place, &bd, &[]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    let text = stdout_of(&out);
    let bd_count: Vec<&str> = text.lines().filter(|line| line.starts_with("[BD-COUNT] ")).collect();
    assert_eq!(bd_count, ["[BD-COUNT] open=3 in_progress=1 blocked=1"], "closed は open にも in_progress にも数えない: {text}");
    assert_eq!(
        memo_rows(&text),
        [
            "[MEMO-DUE-COUNT] n=0 of=1".to_owned(),
            "[MEMO-DUE-NONE]".to_owned(),
            format!("[MEMO-STALE] s2-910 p=2 age_days=4 updated={four}"),
            "[MEMO-STALE-COUNT] n=1 of=1 unreadable=0".to_owned(),
        ],
        "closed の memo は母集団の外（10 日前 / P1 でも stale に数えない）: {text}"
    );

    let closed_blind = fake_bd(&place.dir, "bd-open-only", &diff_json(&open_rows), 0, 0);
    let blind = stdout_of(&wm_rebrief(&place, &closed_blind, &[]));
    assert_eq!(bd_count, blind.lines().filter(|line| line.starts_with("[BD-COUNT] ")).collect::<Vec<&str>>(), "{blind}");
    assert_eq!(memo_rows(&text), memo_rows(&blind), "{blind}");
    assert_ne!(diff_rows(&text), diff_rows(&blind), "変わるのは [DIFF] だけ: {text}");
    fs::remove_dir_all(&place.dir).ok();
}

// ─────────────── 台帳の棚卸し（memo の判定点と齢・設計 ledger-triage.md §7 / §9 (a)） ───────────────

/// memo の label（設計 §3 の弁別）。
const MEMO_LABEL: &str = "[\"intake:memo\"]";

/// いまから `days` 日前の `updated_at`（閾値 3 日の ± 1 日で使う・壁時計の等号を pin しない）。
fn days_ago(days: u64) -> String {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default();
    vessel::fleet::cli::format_utc(now.saturating_sub(days.saturating_mul(86_400)))
}

/// 台帳の 1 要素（open・`labels` / `deps` は JSON の字面・`updated` が `None` なら key を持たない）。
fn memo_issue(id: &str, labels: &str, priority: u64, updated: Option<&str>, deps: &[(&str, &str, &str)]) -> String {
    let updated = updated.map(|ts| format!(",\"updated_at\":\"{ts}\"")).unwrap_or_default();
    let deps: Vec<String> = deps
        .iter()
        .map(|(to, status, kind)| format!("{{\"id\":\"{to}\",\"status\":\"{status}\",\"dependency_type\":\"{kind}\",\"description\":\"本文は読まない\"}}"))
        .collect();
    format!(
        "{{\"id\":\"{id}\",\"title\":\"[memo] t\",\"status\":\"open\",\"priority\":{priority},\"labels\":{labels}{updated},\"dependencies\":[{}]}}",
        deps.join(",")
    )
}

/// memo の場所（自席の退避物なし・打刻 1・`issues` を返す偽 bd）。
fn memo_place(issues: &[String]) -> (WmPlace, String) {
    let place = wm_place();
    wm_stamp(&place, &["sid-memo"]);
    let bd = fake_bd(&place.dir, "bd-memo", &format!("[{}]\n", issues.join(",\n")), 0, 0);
    (place, bd)
}

/// stdout の `[MEMO-` 行。
fn memo_rows(text: &str) -> Vec<String> {
    text.lines().filter(|line| line.starts_with("[MEMO-")).map(str::to_owned).collect()
}

/// (1)(2)(3)(4)(5)(6) 判定点を全部過ぎた memo だけが DUE・依存なし（parent-child だけを含む）∧ P ≤ 2 ∧ 齢 ≥ 3 日だけが
/// STALE・label の無い bead は母集団の外・列は id の数字順（`s2-9` < `s2-13`）。
#[test]
fn seat_rebrief_memo_lists_due_and_stale_memos_against_the_memo_population() {
    let (fresh, two, four, five, ten) = (days_ago(0), days_ago(2), days_ago(4), days_ago(5), days_ago(10));
    let issues = [
        memo_issue("s2-10", MEMO_LABEL, 1, Some(&fresh), &[("s2-2", "closed", "blocks"), ("s2-1", "closed", "blocks")]),
        memo_issue("s2-11", MEMO_LABEL, 2, Some(&four), &[("s2-1", "closed", "blocks"), ("s2-3", "open", "blocks")]),
        memo_issue("s2-12", MEMO_LABEL, 3, Some(&four), &[("s2-e", "closed", "parent-child")]),
        memo_issue("s2-13", MEMO_LABEL, 2, Some(&four), &[]),
        memo_issue("s2-14", MEMO_LABEL, 3, Some(&four), &[]),
        memo_issue("s2-15", MEMO_LABEL, 2, Some(&two), &[]),
        memo_issue("s2-9", MEMO_LABEL, 2, Some(&five), &[("s2-e", "closed", "parent-child")]),
        memo_issue("s2-16", "[\"x\"]", 0, Some(&ten), &[("s2-1", "closed", "blocks")]),
        memo_issue("s2-17", "[]", 0, Some(&ten), &[]),
    ];
    let (place, bd) = memo_place(&issues);
    let out = wm_rebrief(&place, &bd, &[]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    let text = stdout_of(&out);
    assert_eq!(
        memo_rows(&text),
        [
            format!("[MEMO-DUE] s2-10 p=1 blocks=s2-1,s2-2 updated={fresh}"),
            "[MEMO-DUE-COUNT] n=1 of=7".to_owned(),
            format!("[MEMO-STALE] s2-9 p=2 age_days=5 updated={five}"),
            format!("[MEMO-STALE] s2-13 p=2 age_days=4 updated={four}"),
            "[MEMO-STALE-COUNT] n=2 of=7 unreadable=0".to_owned(),
        ],
        "{text}"
    );
    assert!(text.contains("[BD-COUNT] open=9 in_progress=0 blocked=0\n"), "母集団 9 件の台帳: {text}");
    fs::remove_dir_all(&place.dir).ok();
}

/// (7) memo が在っても判定に当たらない周・台帳が空の周は、件数行 + `-NONE` を両方出す（「0 件」を確認した印）。
#[test]
fn seat_rebrief_memo_marks_none_for_both_when_nothing_is_due_or_stale() {
    let issues = [
        memo_issue("s2-1", MEMO_LABEL, 2, Some(&days_ago(2)), &[]),
        memo_issue("s2-2", MEMO_LABEL, 1, Some(&days_ago(9)), &[("s2-3", "open", "blocks")]),
    ];
    let (place, bd) = memo_place(&issues);
    let text = stdout_of(&wm_rebrief(&place, &bd, &[]));
    let none = |of: usize| {
        vec![
            format!("[MEMO-DUE-COUNT] n=0 of={of}"),
            "[MEMO-DUE-NONE]".to_owned(),
            format!("[MEMO-STALE-COUNT] n=0 of={of} unreadable=0"),
            "[MEMO-STALE-NONE]".to_owned(),
        ]
    };
    assert_eq!(memo_rows(&text), none(2), "{text}");
    let empty = fake_bd(&place.dir, "bd-empty", "[]", 0, 0);
    let zero = stdout_of(&wm_rebrief(&place, &empty, &[]));
    assert_eq!(memo_rows(&zero), none(0), "{zero}");
    fs::remove_dir_all(&place.dir).ok();
}

/// (8) `updated_at` が無い・形の読めない memo は stale に数えず `unreadable=` に数える（判定不能を 0 に潰さない）。
#[test]
fn seat_rebrief_memo_counts_unreadable_updated_at_apart_from_stale() {
    let four = days_ago(4);
    let issues = [
        memo_issue("s2-1", MEMO_LABEL, 2, None, &[]),
        memo_issue("s2-2", MEMO_LABEL, 1, Some("3 日前"), &[]),
        memo_issue("s2-3", MEMO_LABEL, 2, Some(&four), &[]),
    ];
    let (place, bd) = memo_place(&issues);
    let text = stdout_of(&wm_rebrief(&place, &bd, &[]));
    let rows = memo_rows(&text);
    assert_eq!(
        rows.iter().filter(|row| row.starts_with("[MEMO-STALE")).cloned().collect::<Vec<_>>(),
        [format!("[MEMO-STALE] s2-3 p=2 age_days=4 updated={four}"), "[MEMO-STALE-COUNT] n=1 of=3 unreadable=2".to_owned()],
        "{text}"
    );
    fs::remove_dir_all(&place.dir).ok();
}

/// (9) 台帳が読めない周は memo の marker も出さず rc 2（既存の `ledger-unreadable`）・閾値の行が無い rules は `no-rule`。
#[test]
fn seat_rebrief_memo_emits_no_marker_when_ledger_or_rule_is_unreadable() {
    let issues = [memo_issue("s2-1", MEMO_LABEL, 2, Some(&days_ago(4)), &[])];
    let (place, _) = memo_place(&issues);
    let broken = fake_bd(&place.dir, "bd-rc1", &format!("[{}]\n", issues.join(",")), 1, 0);
    let out = wm_rebrief(&place, &broken, &[]);
    assert_eq!((rc_of(&out), stderr_of(&out)), (RC_DATA_BROKEN, rebrief_refusal("ledger-unreadable")));
    assert!(stdout_of(&out).is_empty(), "marker は 0 行: {}", stdout_of(&out));

    let bd = fake_bd(&place.dir, "bd-ok", &format!("[{}]\n", issues.join(",")), 0, 0);
    let timeout_only = fixture(
        &place.dir,
        "timeout-only.toml",
        "schema = 1\n\n[[rule]]\nid = \"seat.ledger_timeout_s\"\nkind = \"LedgerTimeoutS\"\nvalue = 60\n\
         enabled = true\nruling = \"user 2026-09-12T02:01Z\"\nruled_at = \"2026-09-12\"\n",
    );
    let out = wm_rebrief(&place, &bd, &["--rules", &timeout_only]);
    assert_eq!((rc_of(&out), stderr_of(&out)), (RC_DATA_BROKEN, rebrief_refusal("no-rule")));
    assert!(stdout_of(&out).is_empty(), "DATA は 0 行: {}", stdout_of(&out));
    let rules = rebrief_rules(&place, 60);
    let healed = stdout_of(&wm_rebrief(&place, &bd, &["--rules", &rules]));
    assert!(healed.contains("\n[MEMO-STALE-COUNT] n=1 of=1 unreadable=0\n"), "閾値 2 行を足すと出る: {healed}");
    fs::remove_dir_all(&place.dir).ok();
}

/// (10) memo の行は `[BD-COUNT]` の直後に連続して並び、`[BD-INPROGRESS` の前で終わる（marker の宣言順）。
#[test]
fn seat_rebrief_memo_rows_follow_bd_count_directly() {
    let issues = [
        memo_issue("s2-1", MEMO_LABEL, 0, Some(&days_ago(0)), &[("s2-2", "closed", "blocks")]),
        memo_issue("s2-3", MEMO_LABEL, 0, Some(&days_ago(6)), &[]),
    ];
    let (place, bd) = memo_place(&issues);
    let text = stdout_of(&wm_rebrief(&place, &bd, &[]));
    let lines: Vec<&str> = text.lines().collect();
    let at = lines.iter().position(|line| line.starts_with("[BD-COUNT] ")).unwrap_or(usize::MAX);
    let heads: Vec<&str> = lines
        .iter()
        .skip(at.saturating_add(1))
        .take(5)
        .filter_map(|line| line.split(' ').next())
        .collect();
    assert_eq!(
        heads,
        ["[MEMO-DUE]", "[MEMO-DUE-COUNT]", "[MEMO-STALE]", "[MEMO-STALE-COUNT]", "[BD-INPROGRESS-NONE]"],
        "{text}"
    );
    fs::remove_dir_all(&place.dir).ok();
}

// ─────────────────── 現在地の DATA（`[MAIN]` / `[RUN]` / `[SEAT]` / `[WIN]`・設計 §12.2・ADR-0031 §2.2・`s2-07l.326`） ───────────────────

/// 現在地の歯の全桁 sha（`base:` / `sha:` の detail に書く）。
const STATUS_SHA: &str = "abcdef0123456789abcdef0123456789abcdef01";
/// 上の短 sha（12 桁・`[PLUGIN] binary=` と同じ桁）。
const STATUS_SHORT: &str = "abcdef012345";

/// 便の event 1 件（`run` / `bead` 付き・`bead` は `s2-<run>`・`account` は呼び手が struct update で足す）。
fn status_event(ts: &str, kind: vessel::fleet::EventKind, run: &str, stage: Option<vessel::fleet::Stage>, detail: Option<&str>) -> vessel::fleet::Event {
    vessel::fleet::Event {
        schema: vessel::fleet::SCHEMA,
        ts: ts.to_owned(),
        kind,
        run: run.to_owned(),
        bead: format!("s2-{run}"),
        host: "h".to_owned(),
        actor: "machine".to_owned(),
        stage,
        seat: None,
        pid: None,
        detail: detail.map(str::to_owned),
        allowance: None,
        registration: None,
        account: None,
    }
}

/// 登録 row の event 1 件（鍵 = role × anchor ゆえ anchor は `<place.anchor>/<target>`・口座 a1）。
fn status_registered(place: &WmPlace, target: &str, role: vessel::seat::role::Role, model: Option<&str>) -> vessel::fleet::Event {
    let registration = vessel::fleet::Registration {
        role,
        anchor: place.anchor.join(target).display().to_string(),
        target: target.to_owned(),
        sid: None,
        account: "a1".to_owned(),
        launch: "cld".to_owned(),
        model: model.map(str::to_owned),
    };
    vessel::fleet::Event {
        registration: Some(registration),
        ..status_event("2026-09-16T00:00:00Z", vessel::fleet::EventKind::SeatRegistered, "", None, None)
    }
}

/// event log を置く（`<state>/fleet/events.jsonl`・1 行 1 event・lock は取らない＝歯の fixture）。
fn status_log(place: &WmPlace, events: &[vessel::fleet::Event]) {
    let dir = place.state.join("fleet");
    fs::create_dir_all(&dir).ok();
    let body: String = events.iter().map(|event| format!("{}\n", event.to_line())).collect();
    fs::write(dir.join("events.jsonl"), body).ok();
}

/// 現在地の歯の場所（打刻 1・自席の退避物なし・空の台帳の偽 bd）。
fn status_place() -> (WmPlace, String) {
    let place = wm_place();
    wm_stamp(&place, &["sid-now"]);
    let bd = fake_bd(&place.dir, "bd", "[]", 0, 0);
    (place, bd)
}

/// 行頭が `head` の行（出現順）。
fn status_rows(text: &str, head: &str) -> Vec<String> {
    text.lines().filter(|line| line.starts_with(head)).map(str::to_owned).collect()
}

/// 行頭の marker の列（`from` の行から `to` の行まで・両端を含む）。
fn status_heads(text: &str, from: &str, to: &str) -> Vec<String> {
    text.lines()
        .skip_while(|line| !line.starts_with(from))
        .take_while(|line| !line.starts_with(to))
        .filter_map(|line| line.split(' ').next())
        .map(str::to_owned)
        .chain(std::iter::once(to.to_owned()))
        .collect()
}

/// (a) 設計 §12.7 が名指す歯: 偽の event log に Spawned 1 便（`SeatSpawned account=x`・detail `base:<sha>`）と Landed 1 便を
/// 書くと、`[RUN]` には Spawned の 1 便だけが `account=x base=<短 sha>` 付きで出て `[RUN-COUNT] n=1`（`-NONE` は出ない）、
/// 登録 row の席が `[SEAT]` に打刻の state 付きで出る。現在地の段は `[WM-DIRECTIVE-COUNT]`〜`[ORPHAN-*]` の間に
/// marker の宣言順で並ぶ。base に marker が無い（RED）。
#[test]
fn seat_rebrief_lists_live_runs_and_seats() {
    use vessel::fleet::{EventKind, Stage};
    use vessel::seat::role::Role;
    let (place, bd) = status_place();
    let base = format!("base:{STATUS_SHA}");
    status_log(
        &place,
        &[
            status_event("2026-09-16T01:00:00Z", EventKind::RunStage, "r1", Some(Stage::Spawned), Some(&base)),
            vessel::fleet::Event {
                account: Some("x".to_owned()),
                ..status_event("2026-09-16T01:00:01Z", EventKind::SeatSpawned, "r1", None, None)
            },
            status_event("2026-09-16T01:00:02Z", EventKind::RunStage, "r2", Some(Stage::Landed), None),
            status_registered(&place, WM_TARGET, Role::Orchestrator, Some("Opus")),
        ],
    );
    let out = wm_rebrief(&place, &bd, &[]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    let text = stdout_of(&out);
    assert_eq!(
        status_rows(&text, "[RUN"),
        [
            format!("[RUN] id=r1 stage=Spawned account=x base={STATUS_SHORT} updated=2026-09-16T01:00:01Z"),
            "[RUN-COUNT] n=1".to_owned(),
        ],
        "Landed の r2 は出ない・-NONE は出ない: {text}"
    );
    assert_eq!(
        status_rows(&text, "[SEAT"),
        ["[SEAT] target=wm:1 role=orchestrator state=idle account=a1 model=Opus", "[SEAT-COUNT] n=1"],
        "{text}"
    );
    assert_eq!(
        status_heads(&text, "[MAIN]", "[ORPHAN-NONE]"),
        ["[MAIN]", "[RUN]", "[RUN-COUNT]", "[SEAT]", "[SEAT-COUNT]", "[WIN-UNKNOWN]", "[ORPHAN-NONE]"],
        "現在地の段は宣言順で `[ORPHAN-*]` の前: {text}"
    );
    assert!(text.starts_with("[SID] sid-now\n[WM] missing\n"), "他の段は不変: {text}");
    fs::remove_dir_all(&place.dir).ok();
}

/// (b) 登録 row 2 席 + state.jsonl（idle / busy）→ `[SEAT]` 2 行に打刻の state が写り、打刻の無い 3 席目は `state=unknown`
/// （`model` の無い row も `unknown`・ハイフンの値を使わない）。`[SEAT-COUNT] n=3`。
#[test]
fn seat_wm_status_lists_seats_with_state_from_the_stamp() {
    use vessel::seat::role::Role;
    let (place, bd) = status_place();
    write_state(&seat_dir_of(&place.state, "other_2"), StateFix::Busy { age_s: 0 });
    status_log(
        &place,
        &[
            status_registered(&place, WM_TARGET, Role::Orchestrator, Some("Opus")),
            status_registered(&place, "other:2", Role::Orchestrator, None),
            status_registered(&place, "third:3", Role::Orchestrator, Some("Sonnet")),
        ],
    );
    let out = wm_rebrief(&place, &bd, &[]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    let text = stdout_of(&out);
    let mut seats = status_rows(&text, "[SEAT] ");
    seats.sort();
    assert_eq!(
        seats,
        [
            "[SEAT] target=other:2 role=orchestrator state=busy account=a1 model=unknown",
            "[SEAT] target=third:3 role=orchestrator state=unknown account=a1 model=Sonnet",
            "[SEAT] target=wm:1 role=orchestrator state=idle account=a1 model=Opus",
        ],
        "{text}"
    );
    assert_eq!(status_rows(&text, "[SEAT-"), ["[SEAT-COUNT] n=3"], "{text}");
    fs::remove_dir_all(&place.dir).ok();
}

/// (c) anchor が git repo でない周は `[MAIN] sha=unknown origin=unknown porcelain=unknown` で rc 0（DATA の他の段は出る・
/// rc 2 に倒さない）。同じ anchor を repo にすると `sha=<短 sha> porcelain=0`・origin/main が無ければ `origin=unknown`・
/// origin/main を HEAD に置けば `same`・未追跡 file 1 つで `porcelain=1`・commit を積めば `ahead`。
#[test]
fn seat_wm_status_reports_main_as_unknown_when_git_is_absent() {
    let (place, bd) = status_place();
    let main_of = |case: &str| -> String {
        let out = wm_rebrief(&place, &bd, &[]);
        assert_eq!(rc_of(&out), i32::from(RC_OK), "{case}: stderr={}", stderr_of(&out));
        let text = stdout_of(&out);
        assert!(text.starts_with("[SID] sid-now\n"), "{case}: DATA は出る: {text}");
        status_rows(&text, "[MAIN]").join("\n")
    };
    assert_eq!(main_of("非 repo"), "[MAIN] sha=unknown origin=unknown porcelain=unknown reason=head-unreadable");
    let Some(head) = crate::git_repo_at(&place.anchor) else {
        panic!("anchor を repo にできる");
    };
    let short = head.get(..12).unwrap_or_default();
    assert_eq!(main_of("origin/main なし"), format!("[MAIN] sha={short} origin=unknown porcelain=0"));
    assert!(crate::git_out(&place.anchor, &["update-ref", "refs/remotes/origin/main", "HEAD"]).is_some());
    assert_eq!(main_of("same"), format!("[MAIN] sha={short} origin=same porcelain=0"));
    fs::write(place.anchor.join("dirty"), "x\n").ok();
    assert_eq!(main_of("未追跡 1"), format!("[MAIN] sha={short} origin=same porcelain=1"));
    assert!(crate::git_out(&place.anchor, &["add", "-A"]).is_some());
    assert!(crate::git_out(&place.anchor, &["commit", "-q", "-m", "next"]).is_some());
    let next = crate::git_out(&place.anchor, &["rev-parse", "HEAD"]).unwrap_or_default();
    assert_ne!(next, head, "commit が積めた");
    assert_eq!(main_of("ahead"), format!("[MAIN] sha={} origin=ahead porcelain=0", next.get(..12).unwrap_or_default()));
    fs::remove_dir_all(&place.dir).ok();
}

/// (d) 自席の消費済み退避物の `externalized_at` の**前**に Landed 1 便・**後**に Landed 2 便（detail `sha:<sha> main:<sha>` の
/// 1 便と `--pr-cmd` 形〔detail `pr`・sha を持たない〕の 1 便）を偽の event log に書くと、`[WIN]` は後の 2 便だけが時刻順に
/// 出て（`sha=<短 sha>` と `sha=unknown` の両分岐）、前の 1 便は出ない（`[WIN-COUNT] n=2`・母集団 3 便）。
/// `externalized_at` の無い古い消費済みは起点に数えない（読めた最大を取る）。
#[test]
fn seat_wm_status_lists_wins_landed_after_the_last_consumed() {
    use vessel::fleet::{EventKind, Stage};
    let (place, bd) = status_place();
    wm_consumed(&place, "working-memory.sid-1.consumed.md", "schema: 1\nseat: wm:1\nexternalized_at: 2026-09-16T01:00:00Z\n", "", "");
    wm_consumed(&place, "working-memory.sid-0.consumed.md", "schema: 1\nseat: wm:1\n", "", "");
    let landed = format!("sha:{STATUS_SHA} main:{STATUS_SHA}");
    let events = [
        status_event("2026-09-16T00:30:00Z", EventKind::RunDone, "w0", Some(Stage::Landed), Some(&landed)),
        status_event("2026-09-16T02:00:00Z", EventKind::RunDone, "w2", Some(Stage::Landed), Some("pr")),
        status_event("2026-09-16T01:30:00Z", EventKind::RunDone, "w1", Some(Stage::Landed), Some(&landed)),
    ];
    assert_eq!(events.iter().filter(|event| event.stage == Some(Stage::Landed)).count(), 3, "母集団 3 便");
    status_log(&place, &events);
    let out = wm_rebrief(&place, &bd, &[]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    let text = stdout_of(&out);
    assert_eq!(
        status_rows(&text, "[WIN"),
        [
            format!("[WIN] id=s2-w1 landed=2026-09-16T01:30:00Z sha={STATUS_SHORT}"),
            "[WIN] id=s2-w2 landed=2026-09-16T02:00:00Z sha=unknown".to_owned(),
            "[WIN-COUNT] n=2".to_owned(),
        ],
        "前の w0 は出ない・-NONE / -UNKNOWN は出ない: {text}"
    );
    assert!(!text.contains("s2-w0"), "起点より前の便: {text}");
    assert_eq!(status_rows(&text, "[RUN"), ["[RUN-COUNT] n=0", "[RUN-NONE]"], "Landed は走行中でない: {text}");
    fs::remove_dir_all(&place.dir).ok();
}

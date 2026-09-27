// flip-check: moved s2-07l.685
//! 純移動の機械証明と lens に渡す diff の畳みの族の歯（接頭辞 `pipe_gate_move_` / `pipe_gate_elide_`・設計 docs/design/carry-prep.md §10 行 m）。
//!
//! 共有の helper と const と外形 snapshot の歯は親 module（`tests/e2e/pipe/gate.rs`）に在り、`use super::*` で使う。
//! 歯の本文は親から**挙動不変で移した**もの（`s2-07l.685`）。

use super::*;

/// (i) 純移動の便は lens の入力が**要約**になる: 判定行 `lens-input=summary bytes=<要約の byte>`・`lens-input.txt` が
/// 在り先頭行が名乗る・fake lens が読んだ stdin は残した本文そのもの・verdict が読める・`diff_bytes` は diff の byte
/// のまま・stderr に理由の行は出ない。
#[test]
fn pipe_gate_move_proof_pure_move_sends_summary() {
    let (repo, state, id) = move_run(&[("lib.rs", MOVE_BASE_LIB)], &move_head());
    let seen = state.join("lens-stdin");
    let out = gate_once(&repo, &state, &id, Some(&recording_lens(&seen)));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&out));
    let line = stdout_of(&out);
    assert_eq!(token_of(&line, "lens-input="), "summary", "判定行: {line}");
    let kept = fs::read_to_string(lens_input_path(&state, &id)).expect("lens-input.txt が在る");
    assert_eq!(kept.lines().next(), Some(SUMMARY_HEADLINE), "先頭行が名乗る: {kept}");
    let received = fs::read_to_string(&seen).expect("lens が読んだ stdin を読める");
    assert_eq!(received, kept, "lens が読んだ stdin は残した本文そのもの");
    assert_eq!(received.lines().next(), Some(SUMMARY_HEADLINE), "stdin の先頭行も名乗る");
    assert_eq!(token_of(&line, "bytes="), kept.len().to_string(), "bytes= は要約の byte: {line}");
    let pairs = verdict_pairs(&state, &id);
    assert_eq!(value_of(&pairs, "verdict"), "PASS", "verdict が読める");
    assert_eq!(value_of(&pairs, "evidence"), "fake", "lens の evidence を写す");
    assert_eq!(value_of(&pairs, "diff_bytes"), raw_diff_len(&repo, &id).to_string(), "diff_bytes は diff の byte のまま");
    assert_ne!(value_of(&pairs, "diff_bytes"), kept.len().to_string(), "要約の byte ではない");
    assert_eq!(stderr_of(&out), "", "純移動の周は理由の行を出さない");
    assert_summary_moves(&kept);
    assert_summary_residual(&kept);
    assert!(!kept.contains("carried markers"), "持ち越した札 0 の周は行を出さない: {kept}");
    clean(&[&repo, &state]);
}

/// (ii) 本文を 1 行変えた fixture は純移動でなく diff が渡る（`items-differ`）。
#[test]
fn pipe_gate_move_proof_body_change_sends_diff() {
    let changed = MOVE_HEAD_BETA.replace("    3\n", "    4\n");
    assert_ne!(changed, MOVE_HEAD_BETA, "fixture は本文が 1 行違う");
    assert_sends_diff(&[("lib.rs", MOVE_HEAD_LIB), ("alpha.rs", MOVE_HEAD_ALPHA), ("beta.rs", &changed)], "items-differ");
}

/// (iii) 宣言と札とコメント以外の行が残差分に残る fixture も diff（`residual-line`）。
#[test]
fn pipe_gate_move_proof_residual_line_sends_diff() {
    let noisy = MOVE_HEAD_LIB.replace("mod alpha;\n", "#![allow(dead_code)]\nmod alpha;\n");
    assert_ne!(noisy, MOVE_HEAD_LIB, "fixture は宣言でない行を 1 つ持つ");
    assert_sends_diff(&[("lib.rs", &noisy), ("alpha.rs", MOVE_HEAD_ALPHA), ("beta.rs", MOVE_HEAD_BETA)], "residual-line");
}

/// (iv) 宣言だけの fixture（item は 1 つも動かない）は純移動でない（`nothing-moved`）。
#[test]
fn pipe_gate_move_proof_zero_moved_items_sends_diff() {
    let declared = format!("mod alpha;\nmod beta;\n\n{MOVE_BASE_LIB}");
    assert_sends_diff(&[("lib.rs", &declared), ("alpha.rs", "//! alpha.\n"), ("beta.rs", "//! beta.\n")], "nothing-moved");
}

/// (v) `// flip-check: retroactive` の札が残差分に在る fixture は diff（lens v2 medium・`foreign-marker`）。
#[test]
fn pipe_gate_move_proof_retroactive_marker_sends_diff() {
    let marked = MOVE_HEAD_BETA.replace("//! beta.\n\n", "//! beta.\n\n// flip-check: retroactive s2-07l.261\n");
    assert_ne!(marked, MOVE_HEAD_BETA, "fixture は retroactive の札を持つ");
    assert_sends_diff(&[("lib.rs", MOVE_HEAD_LIB), ("alpha.rs", MOVE_HEAD_ALPHA), ("beta.rs", &marked)], "foreign-marker");
}

/// (vi) 予算の照合は lens に渡す本文の byte で行う: 要約は cap 内・diff は cap 超の fixture が PASS/FAIL の判定へ
/// 進み INCONCLUSIVE にならず、`verdict.json` の `diff_bytes` は diff の byte（cap 超）のまま。
#[test]
fn pipe_gate_move_proof_budget_uses_summary_bytes() {
    let base_lib = format!("//! big.\n\n{}", big_fn(""));
    let head_lib = "//! big.\n\nmod alpha;\n".to_owned();
    let head_alpha = format!("//! alpha.\n\n{}", big_fn("pub(super) "));
    let (repo, state, id) = move_run(&[("lib.rs", &base_lib)], &[("lib.rs", &head_lib), ("alpha.rs", &head_alpha), ("beta.rs", "//! beta.\n")]);
    let cap = 4_000;
    let diff_len = raw_diff_len(&repo, &id);
    assert!(diff_len > cap, "前提: diff は cap 超（{diff_len} byte）");
    let rules = write_rules(&repo, "summary-cap.toml", 1, cap as u64);
    let marker = state.join("lens-ran");
    let out = gate_with_rules(&repo, &state, &id, &rules, &fake_lens(&marker, &lens_verdict("PASS")));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "判定へ進む: {}", stderr_of(&out));
    let line = stdout_of(&out);
    assert_eq!(token_of(&line, "verdict="), "PASS", "{line}");
    assert_eq!(token_of(&line, "lens-input="), "summary", "{line}");
    let bytes: usize = token_of(&line, "bytes=").parse().unwrap_or(usize::MAX);
    assert!(bytes <= cap, "要約は cap 内: {line}");
    assert!(marker.exists(), "lens を起動した");
    let pairs = verdict_pairs(&state, &id);
    assert_eq!(value_of(&pairs, "verdict"), "PASS", "INCONCLUSIVE にならない");
    assert!(!value_of(&pairs, "evidence").contains("cap"), "cap の理由が無い: {}", value_of(&pairs, "evidence"));
    assert_eq!(value_of(&pairs, "diff_bytes"), diff_len.to_string(), "diff_bytes は diff の byte のまま");
    let kept = fs::read_to_string(lens_input_path(&state, &id)).expect("lens-input.txt が在る");
    assert_eq!(kept.len(), bytes, "bytes= は残した要約の byte");
    assert!(kept.contains("src/lib.rs -> src/alpha.rs: items=1 lines=123\n  fn big\n"), "{kept}");
    clean(&[&repo, &state]);
}

/// (viii) 移した item の doc コメントの link path だけを書き換えた便（`[`super::one`]` → `[`crate::one`]`）は
/// 純移動: 判定行 `lens-input=summary`・要約に「コメント行の差」の節（該当 item の名と行数の直後に base 側 `-` /
/// head 側 `+` の逐語・設計 §25・`s2-07l.377`）・他の面（移動・可視性・stderr）は (i) と同じ。
#[test]
fn pipe_gate_move_proof_comment_only_diff_inside_items_sends_summary() {
    let (base, alpha) = (linked(MOVE_BASE_LIB, "super::one"), linked(MOVE_HEAD_ALPHA, "crate::one"));
    let (repo, state, id) = move_run(&[("lib.rs", &base)], &[("lib.rs", MOVE_HEAD_LIB), ("alpha.rs", &alpha), ("beta.rs", MOVE_HEAD_BETA)]);
    let seen = state.join("lens-stdin");
    let out = gate_once(&repo, &state, &id, Some(&recording_lens(&seen)));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&out));
    let line = stdout_of(&out);
    assert_eq!(token_of(&line, "lens-input="), "summary", "判定行: {line}");
    let kept = fs::read_to_string(lens_input_path(&state, &id)).expect("lens-input.txt が在る");
    assert_eq!(fs::read_to_string(&seen).unwrap_or_default(), kept, "lens が読んだ stdin は残した本文そのもの");
    assert!(
        kept.contains("\n## コメント行の差（名: 行数）\nsrc/alpha.rs fn two: 1\n-/// helper two (see [`super::one`]).\n+/// helper two (see [`crate::one`]).\n## 残差分（逐語）\n"),
        "件数の行の直後に base 側 - / head 側 + の逐語: {kept}"
    );
    assert_eq!(kept.matches("helper two").count(), 2, "コメントの字面は - / + の 2 行だけに載る: {kept}");
    assert_eq!(stderr_of(&out), "", "純移動の周は理由の行を出さない");
    assert_summary_moves(&kept);
    assert_summary_residual(&kept);
    clean(&[&repo, &state]);
}

/// (ix) 移した item の中に `// flip-check: retroactive` の札を足した便は diff（`foreign-marker`）＝コメント行の除外が
/// 札まで緩めていない対（(v) の札は残差分・本 fixture の札は fn の本文の中）。
#[test]
fn pipe_gate_move_proof_comment_marker_inside_item_sends_diff() {
    let marked = MOVE_HEAD_ALPHA.replace("    2\n", "    // flip-check: retroactive s2-07l.294\n    2\n");
    assert_ne!(marked, MOVE_HEAD_ALPHA, "fixture は item の中に retroactive の札を持つ");
    assert_sends_diff(&[("lib.rs", MOVE_HEAD_LIB), ("alpha.rs", &marked), ("beta.rs", MOVE_HEAD_BETA)], "foreign-marker");
}

/// (x) コメント行の書き換え + 本文 1 行の書き換えは diff（`items-differ`）＝除外はコメント行だけに閉じる。
#[test]
fn pipe_gate_move_proof_comment_and_body_change_sends_diff() {
    let changed = linked(MOVE_HEAD_ALPHA, "crate::one").replace("    2\n", "    3\n");
    assert!(changed.contains("    3\n"), "fixture は本文も 1 行違う");
    assert_sends_diff(&[("lib.rs", MOVE_HEAD_LIB), ("alpha.rs", &changed), ("beta.rs", MOVE_HEAD_BETA)], "items-differ");
}

/// (xi) base の item に元から在る `retroactive` の札を、その item ごと別 file へ移した便は純移動: 判定行
/// `lens-input=summary`・要約が持ち越した札の本数を 1 行で名乗る（判定行の直前）・札の字面は残差分に載らない
/// （item の中の行）・他の面（移動・可視性・stderr）は (i) と同じ。
#[test]
fn pipe_gate_move_proof_carried_retroactive_marker_sends_summary() {
    let (base, alpha) = (carried(MOVE_BASE_LIB, "s2-07l.1"), carried(MOVE_HEAD_ALPHA, "s2-07l.1"));
    let (repo, state, id) = move_run(&[("lib.rs", &base)], &[("lib.rs", MOVE_HEAD_LIB), ("alpha.rs", &alpha), ("beta.rs", MOVE_HEAD_BETA)]);
    let seen = state.join("lens-stdin");
    let out = gate_once(&repo, &state, &id, Some(&recording_lens(&seen)));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&out));
    let line = stdout_of(&out);
    assert_eq!(token_of(&line, "lens-input="), "summary", "判定行: {line}");
    let kept = fs::read_to_string(lens_input_path(&state, &id)).expect("lens-input.txt が在る");
    assert_eq!(fs::read_to_string(&seen).unwrap_or_default(), kept, "lens が読んだ stdin は残した本文そのもの");
    assert!(kept.contains("\ncarried markers: 1\n判定: 名 + 本文の多重集合が一致 "), "持ち越した札の本数を判定行の直前で名乗る: {kept}");
    assert_eq!(kept.matches("carried markers").count(), 1, "1 行だけ: {kept}");
    assert!(!kept.contains("retroactive"), "item の中の札の字面は要約に載らない: {kept}");
    assert_eq!(stderr_of(&out), "", "純移動の周は理由の行を出さない");
    assert_summary_moves(&kept);
    assert_summary_residual(&kept);
    clean(&[&repo, &state]);
}

/// (xii) 同じ base で HEAD 側の札の id だけを変えた便は diff（`foreign-marker`）＝対は id まで含む字面で取る
/// （持ち越しを装って別の id の札を足す形を通さない・退行の pin）。
#[test]
fn pipe_gate_move_proof_carried_marker_with_a_different_id_sends_diff() {
    let (base, alpha) = (carried(MOVE_BASE_LIB, "s2-07l.1"), carried(MOVE_HEAD_ALPHA, "s2-07l.2"));
    assert_sends_diff_from(&[("lib.rs", &base)], &[("lib.rs", MOVE_HEAD_LIB), ("alpha.rs", &alpha), ("beta.rs", MOVE_HEAD_BETA)], "foreign-marker");
}

/// (xiii) base に在る札を HEAD で落とした便は diff（`foreign-marker`）＝消えた札も対が無い（札を消す変更は純移動でない）。
#[test]
fn pipe_gate_move_proof_carried_dropped_marker_sends_diff() {
    let base = carried(MOVE_BASE_LIB, "s2-07l.1");
    assert_sends_diff_from(&[("lib.rs", &base)], &move_head(), "foreign-marker");
}

/// (a) rename 1 本 + その旧 path を名指す `docs/design/` の md の row 1 行の置換 → lens の stdin に印が在り置換後の row が
/// 無く、header は残り、通知に `elided=1/2`、`bytes=` は畳んだ本文の byte で生 diff より小さく、`diff_bytes` は生 diff のまま。
#[test]
fn pipe_gate_elide_replacement_only_docs_hunk_is_folded() {
    let base = format!("# notes\n\n{}\n", elide_row(ELIDE_OLD));
    let head = format!("# notes\n\n{}\n", elide_row(ELIDE_NEW));
    let gated = elide_gate(&[(ELIDE_NOTES, &base)], &[(ELIDE_NOTES, &head)], true);
    let added = format!("\n+{}\n", elide_row(ELIDE_NEW));
    assert!(gated.raw.contains(&added), "前提: 生 diff は置換後の row を持つ: {}", gated.raw);
    assert!(
        gated.stdin.contains("\n~ rename の置換だけの hunk（-1/+1 行）を省いた\n"),
        "hunk の本文は印 1 行: {}",
        gated.stdin
    );
    assert!(!gated.stdin.contains(&added), "置換後の row は lens に渡らない: {}", gated.stdin);
    assert!(gated.stdin.contains(&format!("+++ b/{ELIDE_NOTES}\n@@ ")), "header は残る: {}", gated.stdin);
    assert!(gated.stdin.contains(&format!("rename to {ELIDE_NEW}\n")), "rename の header も残る: {}", gated.stdin);
    assert_eq!(gated.notices.len(), 1, "通知は 1 行: {:?}", gated.notices);
    assert!(
        gated.notices.iter().all(|line| line.starts_with("# lens-input=diff reason=") && line.ends_with(" elided=1/2")),
        "通知に elided=<hunk 数>/<行数>: {:?}",
        gated.notices
    );
    let bytes = token_of(&gated.line, "bytes=");
    assert_eq!(bytes, gated.stdin.len().to_string(), "bytes= は lens に渡した本文の byte: {}", gated.line);
    assert!(gated.stdin.len() < gated.raw.len(), "畳んだ本文は生 diff より小さい");
    let pairs = verdict_pairs(&gated.state, &gated.id);
    assert_eq!(value_of(&pairs, "diff_bytes"), gated.raw.len().to_string(), "diff_bytes は生 diff の byte のまま");
    assert_eq!(value_of(&pairs, "verdict"), "PASS", "lens の verdict");
    clean(&[&gated.repo, &gated.state]);
}

/// (b) 同じ hunk に path 以外の 1 語の差も在る → 逐語のまま・通知に `elided=` が無い（置換後の一致の歯）。
#[test]
fn pipe_gate_elide_one_extra_word_keeps_the_hunk_verbatim() {
    let base = format!("# notes\n\n{}\n", elide_row(ELIDE_OLD));
    let worded = elide_row(ELIDE_NEW).replace("the table", "a table");
    let head = format!("# notes\n\n{worded}\n");
    let gated = elide_gate(&[(ELIDE_NOTES, &base)], &[(ELIDE_NOTES, &head)], true);
    assert_elide_verbatim(&gated, &format!("\n+{worded}\n"), "1 語の差");
}

/// (c) 同じ置換が `.rs` の hunk と `docs/design/` の外の `.md` の hunk に在る → どちらも逐語のまま（path の絞りの歯）。
#[test]
fn pipe_gate_elide_code_and_outside_docs_keep_the_hunk_verbatim() {
    let code = |path: &str| format!("// uses {path}\n");
    let guide = |path: &str| format!("# guide\n\n{}\n", elide_row(path));
    let (code_old, code_new) = (code(ELIDE_OLD), code(ELIDE_NEW));
    let (guide_old, guide_new) = (guide(ELIDE_OLD), guide(ELIDE_NEW));
    let gated = elide_gate(
        &[("src/user.rs", &code_old), ("docs/guide.md", &guide_old)],
        &[("src/user.rs", &code_new), ("docs/guide.md", &guide_new)],
        true,
    );
    assert!(gated.stdin.contains(&format!("\n+// uses {ELIDE_NEW}\n")), ".rs の hunk は逐語: {}", gated.stdin);
    assert_elide_verbatim(&gated, &format!("\n+{}\n", elide_row(ELIDE_NEW)), "畳みの面の外");
}

/// (d) rename の header が無い diff → 置換に見える docs の hunk も逐語で、通知と `bytes=` は従来の字面のまま（回帰の歯）。
#[test]
fn pipe_gate_elide_without_rename_keeps_the_former_form() {
    let base = format!("# notes\n\n{}\n", elide_row(ELIDE_OLD));
    let head = format!("# notes\n\n{}\n", elide_row(ELIDE_NEW));
    let gated = elide_gate(&[(ELIDE_NOTES, &base)], &[(ELIDE_NOTES, &head)], false);
    assert_elide_verbatim(&gated, &format!("\n+{}\n", elide_row(ELIDE_NEW)), "rename 無し");
}

/// (e) 生 diff は cap 超・畳んだ本文は cap 内 → INCONCLUSIVE でなく lens が呼ばれ verdict は lens の値
/// （本節の出所の形）・`diff_bytes` は生 diff の byte（cap 超）のまま。
#[test]
fn pipe_gate_elide_folded_body_within_cap_calls_the_lens() {
    let rows = |path: &str| -> String {
        (0..80)
            .map(|number| format!("| row {number:02} names `{path}` and carries enough words to weigh on the cap |\n"))
            .collect()
    };
    let (base, head) = (rows(ELIDE_OLD), rows(ELIDE_NEW));
    let (repo, state, id) = elide_run(&[(ELIDE_NOTES, &base)], &[(ELIDE_NOTES, &head)], true);
    let cap = 4_000;
    let raw = raw_diff(&repo, &id);
    assert!(raw.len() > cap, "前提: 生 diff は cap 超（{} byte）", raw.len());
    let rules = write_rules(&repo, "elide-cap.toml", 1, cap as u64);
    let seen = state.join("lens-stdin");
    let out = gate_with_rules(&repo, &state, &id, &rules, &recording_lens(&seen));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "判定へ進む: {}", stderr_of(&out));
    let line = stdout_of(&out);
    assert_eq!(token_of(&line, "verdict="), "PASS", "{line}");
    let bytes: usize = token_of(&line, "bytes=").parse().unwrap_or(usize::MAX);
    assert!(bytes <= cap, "畳んだ本文は cap 内: {line}");
    let received = fs::read_to_string(&seen).unwrap_or_default();
    assert_eq!(received.len(), bytes, "lens を起動し、畳んだ本文を渡した");
    assert!(received.contains("（-80/+80 行）を省いた\n"), "80 row の hunk を畳んだ: {received}");
    let pairs = verdict_pairs(&state, &id);
    assert_eq!(value_of(&pairs, "verdict"), "PASS", "INCONCLUSIVE にならない");
    assert!(!value_of(&pairs, "evidence").contains("cap"), "cap の理由が無い: {}", value_of(&pairs, "evidence"));
    assert_eq!(value_of(&pairs, "diff_bytes"), raw.len().to_string(), "diff_bytes は生 diff の byte のまま");
    clean(&[&repo, &state]);
}

/// (f) rename を含む diff で docs の md の hunk が行の並べ替えだけ（`-X` / context / `+X`・置換が効かない）→ 逐語
/// （便 161614Z の finding の形・効きと 1 塊の両方を外して初めて落ちる回帰の歯）。
#[test]
fn pipe_gate_elide_reorder_only_hunk_is_verbatim() {
    let base = "X plain line\nC context line\n";
    let head = "C context line\nX plain line\n";
    let gated = elide_gate(&[(ELIDE_NOTES, base)], &[(ELIDE_NOTES, head)], true);
    assert!(
        gated.raw.contains("\n-X plain line\n C context line\n+X plain line\n"),
        "前提: 並べ替えの hunk の形: {}",
        gated.raw
    );
    assert_elide_verbatim(&gated, "\n+X plain line\n", "並べ替え");
}

/// (g) 置換が効く行と効かない行が同じ 1 塊に混在（`-row(旧)` `-X` / `+row(新)` `+X`・X は末尾の改行の有無だけが違う）
/// → 逐語（各行の置換の効きの歯）。
#[test]
fn pipe_gate_elide_mixed_effect_hunk_is_verbatim() {
    let base = format!("{}\nX tail line", elide_row(ELIDE_OLD));
    let head = format!("{}\nX tail line\n", elide_row(ELIDE_NEW));
    let gated = elide_gate(&[(ELIDE_NOTES, &base)], &[(ELIDE_NOTES, &head)], true);
    let shape = format!(
        "\n-{}\n-X tail line\n\\ No newline at end of file\n+{}\n+X tail line\n",
        elide_row(ELIDE_OLD),
        elide_row(ELIDE_NEW)
    );
    assert!(gated.raw.contains(&shape), "前提: 効く行と効かない行の 1 塊: {}", gated.raw);
    assert_elide_verbatim(&gated, "\n+X tail line\n", "効きの混在");
}

/// (h) `-` の各行は置換で変わるが `-` と `+` の間に context が在る（置換を伴う行の移動）→ 逐語（1 塊の歯）。
#[test]
fn pipe_gate_elide_context_between_minus_and_plus_is_verbatim() {
    let base = format!("{}\nC context line\n", elide_row(ELIDE_OLD));
    let head = format!("C context line\n{}\n", elide_row(ELIDE_NEW));
    let gated = elide_gate(&[(ELIDE_NOTES, &base)], &[(ELIDE_NOTES, &head)], true);
    let shape = format!("\n-{}\n C context line\n+{}\n", elide_row(ELIDE_OLD), elide_row(ELIDE_NEW));
    assert!(gated.raw.contains(&shape), "前提: - と + の間に context: {}", gated.raw);
    assert_elide_verbatim(&gated, &format!("\n+{}\n", elide_row(ELIDE_NEW)), "context を挟む移動");
}

/// (i) 2 段の hunk（`-A` / `+A'` / context / `-B` / `+B'`・どちらの段も置換だけ）→ 畳む・通知に `elided=1/4`（段の切り分けの歯）。
#[test]
fn pipe_gate_elide_two_stage_hunk_is_folded() {
    let doc = |path: &str| format!("{}\nC context line\n{}\n", elide_row(path), elide_next_row(path));
    let gated = elide_gate(&[(ELIDE_NOTES, &doc(ELIDE_OLD))], &[(ELIDE_NOTES, &doc(ELIDE_NEW))], true);
    let shape = format!(
        "\n-{}\n+{}\n C context line\n-{}\n+{}\n",
        elide_row(ELIDE_OLD),
        elide_row(ELIDE_NEW),
        elide_next_row(ELIDE_OLD),
        elide_next_row(ELIDE_NEW)
    );
    assert!(gated.raw.contains(&shape), "前提: 1 つの hunk に 2 段: {}", gated.raw);
    let kept = format!("\n+{}\n", elide_next_row(ELIDE_NEW));
    assert_elide_folded(&gated, &kept, "-2/+2", " elided=1/4");
}

/// (j) 段の本数が違う（`-A` / `-B` / `+A'`・どちらの `-` も置換で変わる）→ 逐語（列の相等で落ちることを固定する回帰の歯）。
#[test]
fn pipe_gate_elide_stage_count_mismatch_is_verbatim() {
    let base = format!("{}\n{}\n", elide_row(ELIDE_OLD), elide_next_row(ELIDE_OLD));
    let head = format!("{}\n", elide_row(ELIDE_NEW));
    let gated = elide_gate(&[(ELIDE_NOTES, &base)], &[(ELIDE_NOTES, &head)], true);
    let shape = format!("\n-{}\n-{}\n+{}\n", elide_row(ELIDE_OLD), elide_next_row(ELIDE_OLD), elide_row(ELIDE_NEW));
    assert!(gated.raw.contains(&shape), "前提: -2/+1 の段: {}", gated.raw);
    assert_elide_verbatim(&gated, &format!("\n+{}\n", elide_row(ELIDE_NEW)), "段の本数の違い");
}

/// (k) HEAD に `tests/` 配下の path が無く、docs の行が `tests/` の dir だけを名指す置換 → 畳む（dir の対の導出の歯）。
#[test]
fn pipe_gate_elide_emptied_dir_replacement_is_folded() {
    let (base, head) = (format!("{}\n", elide_dir_row("tests")), format!("{}\n", elide_dir_row("boundary/tests")));
    let gated = elide_moves_gate(&[(ELIDE_NOTES, &base)], &[(ELIDE_NOTES, &head)], &[ELIDE_TESTS_MOVE]);
    let listed = git(&worktree_of(&gated.repo, &gated.id), &["ls-tree", "-r", "--name-only", "HEAD", "tests"]);
    assert_eq!(listed, "", "前提: HEAD の tests/ 配下に path が無い");
    let kept = format!("\n+{}\n", elide_dir_row("boundary/tests"));
    assert_elide_folded(&gated, &kept, "-1/+1", " elided=1/2");
}

/// (l) (k) と同じで HEAD に `tests/` 配下の path が 1 つ残る → 逐語（空の条件の歯・dir の対を足さない）。
#[test]
fn pipe_gate_elide_dir_with_a_remaining_path_is_verbatim() {
    let (base, head) = (format!("{}\n", elide_dir_row("tests")), format!("{}\n", elide_dir_row("boundary/tests")));
    let gated = elide_moves_gate(
        &[(ELIDE_NOTES, &base), ("tests/keep.rs", "// stays under tests\n")],
        &[(ELIDE_NOTES, &head)],
        &[ELIDE_TESTS_MOVE],
    );
    let listed = git(&worktree_of(&gated.repo, &gated.id), &["ls-tree", "-r", "--name-only", "HEAD", "tests"]);
    assert_eq!(listed, "tests/keep.rs", "前提: HEAD の tests/ 配下に 1 本残る");
    assert_elide_verbatim(&gated, &format!("\n+{}\n", elide_dir_row("boundary/tests")), "配下に path が残る dir");
}

/// (m) (k) と同じで `tests/` 配下のもう 1 本の rename が別の dir へ行く → 逐語（一貫の条件の歯）。もう 1 本は file 名も
/// 変える（それ自身から `tests` の対が導かれない＝一貫の条件だけが `tests` の対を塞ぐ形）。
#[test]
fn pipe_gate_elide_dir_whose_renames_diverge_is_verbatim() {
    let (base, head) = (format!("{}\n", elide_dir_row("tests")), format!("{}\n", elide_dir_row("boundary/tests")));
    let gated = elide_moves_gate(
        &[(ELIDE_NOTES, &base)],
        &[(ELIDE_NOTES, &head)],
        &[ELIDE_TESTS_MOVE, ("tests/other.rs", "elsewhere/renamed.rs")],
    );
    let listed = git(&worktree_of(&gated.repo, &gated.id), &["ls-tree", "-r", "--name-only", "HEAD", "tests"]);
    assert_eq!(listed, "", "前提: HEAD の tests/ 配下に path が無い（空の条件は満たす）");
    assert_elide_verbatim(&gated, &format!("\n+{}\n", elide_dir_row("boundary/tests")), "配下の rename が別の dir へ行く");
}

// flip-check: moved s2-07l.685
//! 純移動の機械証明と lens に渡す diff の畳みの族の歯（接頭辞 `pipe_gate_move_` / `pipe_gate_elide_` / `pipe_gate_prune_`・設計 docs/design/carry-prep.md §10 行 m）。
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

// ───── cap を超える周の削除の run の畳み（設計 gate-cost.md §46・行 aq・接頭辞 `pipe_gate_prune_`） ─────
//
// 削除の toy は `PruneToy` で作る（呼ぶたびに新しい toy repo と state）。1 つの歯で 2 つ以上の周を測るときは、同じ fixture の
// 便を別の toy でもう 1 本作って gate する（gate を撃てるのは `Implemented` か INCONCLUSIVE の `Gated` だけ）。

/// 削除の歯の toy の定義（base の file 群・HEAD の写し・runner が `git rm` する file・rename の有無）。
struct PruneToy {
    /// base に commit する file（path, 本文）。runner が消す file と書き換える file の両方を含み、全部 write-set に素の path で載る。
    base: Vec<(String, String)>,
    /// runner が HEAD へ写す file（path, 本文）。
    head: Vec<(String, String)>,
    /// runner が `git rm` する file。
    removed: Vec<String>,
    /// rename の対（[`ELIDE_OLD`] → [`ELIDE_NEW`]）を足すか。置換の hunk は呼び手が base と HEAD の file で渡す。
    rename: bool,
}

/// 削除の歯の gate 1 回の観測。
struct PruneGate {
    /// 判定行・lens の stdin（lens が呼ばれない周は空）・生 diff・通知の行。
    gated: ElideGate,
    /// gate の rc。
    rc: i32,
}

/// 頭（字下げ 0）と閉じ（字下げ 0）と字下げ 4 の本文 `lines - 2` 行の塊（`tag` で行ごとに字面が違い、本文の行は 20 byte 以上）。
fn prune_block(tag: &str, lines: usize) -> String {
    let body: String = (0..lines.saturating_sub(2))
        .map(|number| format!("    {tag} body line {number:02} with some filler words\n"))
        .collect();
    format!("block {tag} {{\n{body}}}\n")
}

/// 前の context の 3 行（`keep <side> <n>`）。
fn prune_keeps(side: &str, count: usize) -> String {
    (0..count).map(|number| format!("keep {side} {number}\n")).collect()
}

/// 1 file を丸ごと消す toy（`lines` 行の塊）。
fn deleted_file_toy(lines: usize) -> PruneToy {
    PruneToy {
        base: vec![("src/gone.txt".to_owned(), prune_block("gone", lines))],
        head: Vec::new(),
        removed: vec!["src/gone.txt".to_owned()],
        rename: false,
    }
}

/// 80 行の row の path を置換する docs の hunk（[`pipe_gate_elide_folded_body_within_cap_calls_the_lens`] の形）。
fn elide_rows(path: &str) -> String {
    (0..80)
        .map(|number| format!("| row {number:02} names `{path}` and carries enough words to weigh on the cap |\n"))
        .collect()
}

/// (a) の fixture: 丸ごと消す file（40 行）と中の 20 行の塊を消す file を、rename の周（§41 で畳む 1 行の置換の hunk）で。
fn whole_and_partial_toy() -> PruneToy {
    let partial = |middle: &str| format!("{}{middle}{}", prune_keeps("a", 6), prune_keeps("b", 6));
    let (notes_old, notes_new) = (format!("# notes\n\n{}\n", elide_row(ELIDE_OLD)), format!("# notes\n\n{}\n", elide_row(ELIDE_NEW)));
    PruneToy {
        base: vec![
            ("src/whole.txt".to_owned(), prune_block("whole", 40)),
            ("src/partial.txt".to_owned(), partial(&prune_block("partial", 20))),
            (ELIDE_NOTES.to_owned(), notes_old),
        ],
        head: vec![("src/partial.txt".to_owned(), partial("")), (ELIDE_NOTES.to_owned(), notes_new)],
        removed: vec!["src/whole.txt".to_owned()],
        rename: true,
    }
}

/// 置き換えの toy: 20 行の塊が、直後の `+` 行（`replaced`）か context を挟んだ `+` 行（`!replaced`）と並ぶ。
fn replaced_toy(replaced: bool) -> PruneToy {
    let base = format!("{}{}", prune_block("old", 20), prune_keeps("a", 3));
    let head = if replaced {
        format!("new line one\nnew line two\n{}", prune_keeps("a", 3))
    } else {
        "keep a 0\nadded line between\nkeep a 1\nkeep a 2\n".to_owned()
    };
    PruneToy {
        base: vec![("src/swap.txt".to_owned(), base)],
        head: vec![("src/swap.txt".to_owned(), head)],
        removed: Vec::new(),
        rename: false,
    }
}

impl PruneToy {
    /// toy repo と state を新しく作り、便を Implemented まで通す: base を commit し、runner は rename と `git rm` と HEAD の写しを
    /// 1 本の commit にする（write-set は rename の対と base の file 群を素の path で）。
    #[expect(
        clippy::expect_used,
        reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
    )]
    fn run(&self) -> (PathBuf, PathBuf, String) {
        let (repo, state) = repo_with_state();
        let mut written: Vec<(&str, &str)> = self.base.iter().map(|(path, body)| (path.as_str(), body.as_str())).collect();
        if self.rename {
            written.push((ELIDE_OLD, "// the renamed module\n"));
        }
        for (path, body) in written {
            let file = repo.join(path);
            fs::create_dir_all(file.parent().expect("file の親が在る")).expect("base の dir を作れる");
            fs::write(&file, body).expect("base の file を書ける");
        }
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-q", "-m", "prune-base"]);
        let staged = state.join("head");
        fs::create_dir_all(&staged).expect("HEAD の写しの dir を作れる");
        let mut steps: Vec<String> = Vec::new();
        if self.rename {
            steps.push(format!("git mv {ELIDE_OLD} {ELIDE_NEW}"));
        }
        steps.extend(self.removed.iter().map(|path| format!("git rm -q {path}")));
        for (index, (path, body)) in self.head.iter().enumerate() {
            let copy = staged.join(index.to_string());
            fs::write(&copy, body).expect("HEAD の file を書ける");
            steps.push(format!("cp '{}' {path}", copy.display()));
        }
        steps.push("git add -A".to_owned());
        steps.push("git commit -q -m runner".to_owned());
        let mut listed: Vec<String> = Vec::new();
        if self.rename {
            listed.push(format!("\"{ELIDE_OLD}\""));
            listed.push(format!("\"+{ELIDE_NEW}\""));
        }
        listed.extend(self.base.iter().map(|(path, _)| format!("\"{path}\"")));
        let write_set = format!("write-set = [{}]", listed.join(", "));
        let design = write_contract(&repo, &["write-set"], &[&write_set]);
        let id = intake(&repo, &state, &design);
        let out = spawn_with(&repo, &state, &id, &steps.join(" && "));
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "spawn: {}", stderr_of(&out));
        (repo, state, id)
    }

    /// 新しい toy で便を作って stdin を写す lens で 1 回 gate する。`cap` は生 diff の byte から rules 行 `gate.token_cap` の値を導く
    /// （`None` は既定の cap）。lens が呼ばれない周の stdin は空。
    fn gate(&self, cap: Option<&dyn Fn(usize) -> usize>) -> PruneGate {
        let (repo, state, id) = self.run();
        let raw = raw_diff(&repo, &id);
        let seen = state.join("lens-stdin");
        let lens = recording_lens(&seen);
        let out = match cap {
            None => gate_once(&repo, &state, &id, Some(&lens)),
            Some(derive) => {
                let value = u64::try_from(derive(raw.len())).unwrap_or(u64::MAX);
                let rules = write_rules(&repo, "prune-cap.toml", 1, value);
                gate_with_rules(&repo, &state, &id, &rules, &lens)
            }
        };
        let line = stdout_of(&out);
        let rc = out.status.code().unwrap_or(-1);
        let gated = ElideGate {
            stdin: fs::read_to_string(&seen).unwrap_or_default(),
            notices: notice_lines(&state, &id),
            raw,
            line,
            repo,
            state,
            id,
        };
        PruneGate { gated, rc }
    }
}

/// 生 diff の byte − 1（(c)(d) の cap・rename が無い toy では §41 の後の本文は生 diff と同じ）。
fn one_below(raw: usize) -> usize {
    raw.saturating_sub(1)
}

/// 畳まなかった周の通知: 1 行で `pruned=` が無い。
fn assert_no_pruned_notice(gated: &ElideGate, why: &str) {
    assert_eq!(gated.notices.len(), 1, "{why}: 通知は 1 行: {:?}", gated.notices);
    assert!(gated.notices.iter().all(|line| !line.contains("pruned=")), "{why}: pruned= が無い: {:?}", gated.notices);
}

/// cap の超過で INCONCLUSIVE になった周: rc 3・lens を呼ばない・evidence に cap の値・`diff_bytes` は生 diff の byte。
fn assert_over_cap(observed: &PruneGate, cap: usize, why: &str) {
    let gated = &observed.gated;
    assert_eq!(observed.rc, i32::from(RC_INCONCLUSIVE), "{why}: cap 超過は rc 3: {}", gated.line);
    assert_eq!(gated.stdin, "", "{why}: lens を起動しない");
    let pairs = verdict_pairs(&gated.state, &gated.id);
    assert_eq!(value_of(&pairs, "verdict"), "INCONCLUSIVE", "{why}");
    assert!(value_of(&pairs, "evidence").contains(&format!("cap {cap} ")), "{why}: evidence に cap: {}", value_of(&pairs, "evidence"));
    assert_eq!(value_of(&pairs, "diff_bytes"), gated.raw.len().to_string(), "{why}: diff_bytes は生 diff の byte");
}

/// 畳まれて lens が呼ばれた周: rc 0・PASS・stdin は lens へ渡した本文で `bytes=` と一致・通知の末尾が `notice`・`diff_bytes` は生 diff。
fn assert_pruned_pass(observed: &PruneGate, notice: &str, why: &str) {
    let gated = &observed.gated;
    assert_eq!(observed.rc, i32::from(RC_OK), "{why}: lens が呼ばれ PASS: {}", gated.line);
    assert_eq!(token_of(&gated.line, "lens-input="), "diff", "{why}: 前提: diff の周: {}", gated.line);
    assert_eq!(token_of(&gated.line, "bytes="), gated.stdin.len().to_string(), "{why}: bytes= は stdin の byte: {}", gated.line);
    assert!(gated.stdin.len() < gated.raw.len(), "{why}: 畳んだ本文は生 diff より小さい");
    assert_eq!(gated.notices.len(), 1, "{why}: 通知は 1 行: {:?}", gated.notices);
    assert!(gated.notices.iter().all(|line| line.ends_with(notice)), "{why}: 通知の末尾 `{notice}`: {:?}", gated.notices);
    let pairs = verdict_pairs(&gated.state, &gated.id);
    assert_eq!(value_of(&pairs, "verdict"), "PASS", "{why}");
    assert_eq!(value_of(&pairs, "diff_bytes"), gated.raw.len().to_string(), "{why}: diff_bytes は生 diff の byte");
}

/// (a) 2 つの file の削除（丸ごと 40 行・中の 20 行）と rename を同じ便で gate し、cap は §41 の畳みの後の本文は超え形 1 の後の
/// 本文は収まる値（生 diff の半分）→ lens の stdin に両 run の字下げ 0 の行と印 2 行と §41 の印が在り、字下げ 4 の本文の行が無い・
/// header は残る・通知の末尾は ` elided=1/2 pruned=2/56`。
#[test]
fn pipe_gate_prune_keeps_only_the_shallowest_lines_of_deletion_runs() {
    let observed = whole_and_partial_toy().gate(Some(&|raw| raw / 2));
    let gated = &observed.gated;
    assert!(gated.raw.contains("-    whole body line 05"), "前提: 生 diff は深い行を持つ: {}", gated.raw);
    assert_pruned_pass(&observed, " elided=1/2 pruned=2/56", "(a)");
    let mark = |run: usize, omitted: usize| format!("~ 削除だけの run（-{run} 行）から字下げの深い行と空行 {omitted} 行を省いた\n");
    assert!(gated.stdin.contains(&format!("\n-block whole {{\n-}}\n{}", mark(40, 38))), "丸ごと消す file の run: {}", gated.stdin);
    assert!(
        gated.stdin.contains(&format!(" keep a 5\n-block partial {{\n-}}\n{} keep b 0\n", mark(20, 18))),
        "中の塊の run（context は残る）: {}",
        gated.stdin
    );
    assert!(gated.stdin.contains("\n~ rename の置換だけの hunk（-1/+1 行）を省いた\n"), "§41 の印: {}", gated.stdin);
    assert!(!gated.stdin.contains("body line"), "字下げ 4 の本文の行は lens に渡らない: {}", gated.stdin);
    assert!(gated.stdin.contains("deleted file mode 100644\n"), "deleted file mode の header は残る: {}", gated.stdin);
    assert!(gated.stdin.contains("\n@@ -1,40 +0,0 @@\n"), "@@ の行は残る: {}", gated.stdin);
    assert!(gated.stdin.len() <= gated.raw.len() / 2, "stdin は cap 以下");
    clean(&[&gated.repo, &gated.state]);
}

/// (b) 同じ歯の中で 3 本の便をそれぞれ別の toy で gate する。(a) と同じ fixture の便を (a) の cap で gate した周は畳む。同じ
/// fixture の便を既定の cap（生 diff が収まる）で gate した周は削除の run が逐語で通知に `pruned=` が無い（cap を超える周だけ）。
/// 3 本目は cap が生 diff の byte より小さく §41 の畳みの後の本文の byte より大きい便で、`pruned=` が無く削除の run が逐語。
#[test]
fn pipe_gate_prune_runs_only_when_the_folded_body_exceeds_the_cap() {
    let over = whole_and_partial_toy().gate(Some(&|raw| raw / 2));
    assert!(over.gated.notices.iter().all(|line| line.contains(" pruned=")), "cap を超える周は畳む: {:?}", over.gated.notices);
    clean(&[&over.gated.repo, &over.gated.state]);

    let within = whole_and_partial_toy().gate(None);
    let gated = &within.gated;
    assert_eq!(within.rc, i32::from(RC_OK), "既定の cap では lens が呼ばれ PASS: {}", gated.line);
    assert_no_pruned_notice(gated, "既定の cap");
    assert!(gated.notices.iter().all(|line| line.ends_with(" elided=1/2")), "§41 の畳みだけ: {:?}", gated.notices);
    assert!(!gated.stdin.contains("~ 削除だけの run"), "削除の run の印が無い: {}", gated.stdin);
    let deleted: Vec<&str> = gated.raw.lines().filter(|line| line.starts_with("-    ")).collect();
    assert_eq!(deleted.len(), 56, "前提: 生 diff の深い削除の行");
    assert!(deleted.iter().all(|line| gated.stdin.contains(&format!("{line}\n"))), "削除の run は逐語: {}", gated.stdin);
    clean(&[&gated.repo, &gated.state]);

    let big = PruneToy {
        base: vec![("src/gone.txt".to_owned(), prune_block("gone", 20)), (ELIDE_NOTES.to_owned(), elide_rows(ELIDE_OLD))],
        head: vec![(ELIDE_NOTES.to_owned(), elide_rows(ELIDE_NEW))],
        removed: vec!["src/gone.txt".to_owned()],
        rename: true,
    };
    let third = big.gate(Some(&|raw| raw / 2));
    let gated = &third.gated;
    assert_eq!(third.rc, i32::from(RC_OK), "§41 の畳みの後は cap に収まり lens が呼ばれる: {}", gated.line);
    assert!(gated.stdin.len() < gated.raw.len() / 2, "前提: §41 の後の本文は cap に収まる");
    assert!(gated.raw.len() / 2 < gated.raw.len(), "前提: cap は生 diff より小さい");
    assert_no_pruned_notice(gated, "cap の照合は §41 の後の本文");
    assert!(gated.notices.iter().all(|line| line.ends_with(" elided=1/160")), "80 row の hunk を畳んだ: {:?}", gated.notices);
    assert!(gated.stdin.contains("（-80/+80 行）を省いた\n"), "§41 の印: {}", gated.stdin);
    let deleted: Vec<&str> = gated.raw.lines().filter(|line| line.starts_with("-    ")).collect();
    assert_eq!(deleted.len(), 18, "前提: 生 diff の深い削除の行");
    assert!(deleted.iter().all(|line| gated.stdin.contains(&format!("{line}\n"))), "削除の run は逐語: {}", gated.stdin);
    clean(&[&gated.repo, &gated.state]);
}

/// (c) 消す run が 15 行の便は、cap が生 diff の byte − 1（畳めば収まり畳まなければ超える値）でも畳まず INCONCLUSIVE（evidence に cap）で
/// 通知に `pruned=` が無い。同じ歯の中で、別の toy の同じ形の 16 行の run の便は畳まれて lens が呼ばれる（本数の線の歯）。
#[test]
fn pipe_gate_prune_does_not_fold_a_run_shorter_than_sixteen_lines() {
    let short = deleted_file_toy(15).gate(Some(&one_below));
    let cap = short.gated.raw.len().saturating_sub(1);
    assert!(short.gated.raw.contains("-    gone body line 05"), "前提: 15 行の run: {}", short.gated.raw);
    assert_over_cap(&short, cap, "15 行");
    assert_no_pruned_notice(&short.gated, "15 行");
    clean(&[&short.gated.repo, &short.gated.state]);

    let long = deleted_file_toy(16).gate(Some(&one_below));
    assert_pruned_pass(&long, " pruned=1/14", "16 行");
    assert!(long.gated.stdin.contains("-block gone {\n-}\n~ 削除だけの run（-16 行）から字下げの深い行と空行 14 行を省いた\n"), "{}", long.gated.stdin);
    clean(&[&long.gated.repo, &long.gated.state]);
}

/// (d) 20 行の削除の run の直後に `+` 行が続く塊（置き換え）の便は、cap が生 diff の byte − 1 でも畳まず INCONCLUSIVE で通知に
/// `pruned=` が無い。同じ歯の中で、別の toy の `-` と `+` の間に context を 1 行挟んだ形の便は畳まれる（直後が `+` 行でない条件の歯）。
#[test]
fn pipe_gate_prune_does_not_fold_a_run_followed_by_added_lines() {
    let swapped = replaced_toy(true).gate(Some(&one_below));
    let cap = swapped.gated.raw.len().saturating_sub(1);
    assert!(swapped.gated.raw.contains("\n-}\n+new line one\n"), "前提: 削除の直後が + 行: {}", swapped.gated.raw);
    assert_over_cap(&swapped, cap, "置き換え");
    assert_no_pruned_notice(&swapped.gated, "置き換え");
    clean(&[&swapped.gated.repo, &swapped.gated.state]);

    let spaced = replaced_toy(false).gate(Some(&one_below));
    assert!(spaced.gated.raw.contains("\n-}\n keep a 0\n+added line between\n"), "前提: 間に context: {}", spaced.gated.raw);
    assert_pruned_pass(&spaced, " pruned=1/18", "context を挟む");
    assert!(spaced.gated.stdin.contains("\n+added line between\n"), "+ 行は残る: {}", spaced.gated.stdin);
    clean(&[&spaced.gated.repo, &spaced.gated.state]);
}

/// (e) 畳んでも cap を超える値は INCONCLUSIVE のまま（lens を呼ばない）。evidence の byte は畳んだ本文の byte（生 diff の byte より小さい）
/// で、通知に `pruned=` が在る（畳みは判定を緩めない）。
#[test]
fn pipe_gate_prune_still_refuses_a_body_that_exceeds_the_cap_after_folding() {
    let observed = deleted_file_toy(40).gate(Some(&|_| 100));
    let gated = &observed.gated;
    assert_over_cap(&observed, 100, "畳んでも超える");
    let omitted: usize = gated.raw.lines().filter(|line| line.starts_with("-    ")).map(|line| line.len() + 1).sum();
    let mark = "~ 削除だけの run（-40 行）から字下げの深い行と空行 38 行を省いた\n".len();
    let folded = gated.raw.len() - omitted + mark;
    assert!(folded < gated.raw.len(), "前提: 畳んだ本文は生 diff より小さい");
    let pairs = verdict_pairs(&gated.state, &gated.id);
    assert!(value_of(&pairs, "evidence").contains(&format!("diff {folded} byte が cap 100")), "evidence の byte は畳んだ本文: {}", value_of(&pairs, "evidence"));
    assert_eq!(gated.notices.len(), 1, "通知は 1 行: {:?}", gated.notices);
    assert!(gated.notices.iter().all(|line| line.ends_with(" pruned=1/38")), "畳んだ周は pruned= が在る: {:?}", gated.notices);
    clean(&[&gated.repo, &gated.state]);
}

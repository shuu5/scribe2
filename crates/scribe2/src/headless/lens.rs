//! `<NAME> lens`（設計 §6・FR9 / NFR1）。契約を `--contract` の path で・diff を stdin で
//! 受け、判定の JSON 1 行を返す。
//!
//! **契約が無ければ claude を呼ばない**。lens に問うのは「diff が契約の求めるものを
//! 満たすか」であって、diff だけを渡して同じことを問うことはできない——実 lens は
//! 「契約が未提供で適合を判定できない」と正しく INCONCLUSIVE を返し、便はそこで止まる
//! （実測 2026-09-10・s2-07l.24 の実 5 便）。**材料の不足は前提違反として断る**（rc 1）。
//! 契約を prompt へ差し込んでから判定を問うのが lens の口である。
//!
//! **`--worktree` は必須である**。lens に憲法（生成区間を持つ `CLAUDE.md`）を載せる経路は
//! 起動 cwd 1 本なので、渡されなければ claude を起こさず rc 1 で断る——継承した cwd に
//! 頼ると、呼び手が変わった周に憲法の載らない判定が静かに出る（fail-closed・C11.2）。
//!
//! **cap を超えた diff では claude を呼ばない**。呼んでから「長すぎた」と言うのでは、
//! 上限を置いた意味（NFR1）が無い。判定に届かなかった周はすべて INCONCLUSIVE へ倒す——
//! 偽の PASS を作らないためである（AC3）。
//!
//! **cap の値は rules 行 `gate.token_cap` からだけ読む**（憲法 C1・`s2-07l.272`）。以前は
//! argv の `--cap` で受けていたので、値の出所が manifest（gate の判定）と launcher の手書き
//! （lens の判定）の 2 つに割れ、rules 行を上げても lens 側は旧値のまま INCONCLUSIVE を
//! 返した（実測 2026-09-14・`.265` / `.267`）。`--rules PATH` が在ればその manifest・無ければ
//! 埋め込み（`pipe::cli` と同じ規約）。`--cap` は**未知の引数として断る**（黙って読み飛ばすと、
//! 手書きの数が残った launcher が「効いている」ように見える）。
//!
//! **model も同じ manifest の rules 行 `runner.model` から読み、claude に毎回渡す**（`s2-07l.297`・
//! 設計 pipeline.md §6）。読み口は [`super::rules_of`] / [`super::runner_model`]（runner と共通）で、
//! 行が解けない周は cap と同じ極性＝claude を呼ばず rc 2。
//!
//! **裁定（便の質問と planner の回答の対）は契約の写しの隣の [`RULINGS_FILE`] から読む**（`s2-07l.309`・
//! 設計 pipeline-question.md）。gate が event log から写す file で、lens は `{contract}` の path の同じ dir
//! から同じ名で引く（env も flag も足さない＝`lens.cmd` の穴は不変）。無ければ「裁定なし」を prompt に明示し
//! （C10・空を黙らせない）、在るのに読めない周は claude を呼ばず rc 2——裁定を落として審査すると、回答で
//! 認めた逸脱が契約違反に読まれ、同じ diff で判定が揺れる（実測 2026-09-15・`.295` の追随周）。
//!
//! **契約の審査（`Stage::Reviewed`・FR49・設計 contract-source.md §4）も同じ口である**。`pipe intake` の直後の
//! 審査は契約の写しを run dir の `review/` に置き、その隣に `{design}` / `{requirements}` の本文
//! （[`DESIGN_FILE`] / [`REQUIREMENTS_FILE`]）を置いて `{contract}` にその写しを渡す。契約の隣にこの 2 file が
//! 在る周は雛形を [`CONTRACT_TEMPLATE`]（diff 無し・観点 3 つ）に切り替え、stdin は読まない（裁定の写しと
//! 同じ「隣の file」の形＝`lens.cmd` の穴も flag も不変で 1 つの `--lens` が 2 つの段に効く）。片方だけ在る周は
//! 材料が壊れているので claude を呼ばず rc 2。cap は契約 + 節 + 要件の byte で照合する（NFR1・超えたら
//! INCONCLUSIVE）。

use super::{build, feed, fill, flag, need, read_stdin_bytes, rules_of, runner_model, Call, DEFAULT_CLAUDE};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
use crate::fleet::select::Model;
use crate::pipe::confine;
use crate::pipe::contract::Contract;
use crate::pipe::move_proof::RULINGS_FILE;
use crate::pipe::review::{DESIGN_FILE, REQUIREMENTS_FILE};
use crate::rules::int_row;
use std::io::ErrorKind;
use std::path::Path;

/// prompt の文面（tracked な template・絶対 path も口座名も含まない）。
const TEMPLATE: &str = include_str!("lens.txt");

/// 契約の審査の prompt の文面（穴 = `{contract}` / `{design}` / `{requirements}`・diff は無い）。
const CONTRACT_TEMPLATE: &str = include_str!("lens-contract.txt");

/// 裁定の file が無い周に `{rulings}` の穴へ入れる 1 行（「裁定なし」を明示する・C10）。
const NO_RULINGS: &str = "（裁定なし）";

/// JSON 行の見出し。
const JSON_HEAD: char = '{';

/// diff の byte 数の上限を持つ rules 行（gate の判定と同じ 1 行・`pipe::cli::ROW_CAP` と同じ id）。
const ROW_CAP: &str = "gate.token_cap";

/// lens が受ける flag の全部（この外は未知の引数として断る）。
const KNOWN_FLAGS: [&str; 6] = ["--contract", "--worktree", "--permission-mode", "--rules", "--account-dir", "--claude"];

/// 使い方の 1 行。
pub fn usage() -> String {
    format!(
        "usage: {} lens --contract F --worktree D --permission-mode M [--rules PATH] [--account-dir D] [--claude PATH] < diff",
        crate::name::NAME
    )
}

/// 判定に届かなかった 1 行を組む。
fn inconclusive(reason: &str) -> String {
    format!(r#"{{"verdict":"INCONCLUSIVE","evidence":"{reason}"}}"#)
}

/// [`KNOWN_FLAGS`] の外の引数を 1 つ名指す（無ければ `None`）。
///
/// flag の次の 1 語は値として飛ばす（`--` で始まる語は値でなく次の flag として読む＝値欠けは
/// [`flag`] が「値が無い」で断る）。撤去した `--cap` をここで捕まえる＝launcher の手書きは構造で止まる。
fn unknown_arg(args: &[String]) -> Option<&str> {
    let mut at = 0;
    while let Some(arg) = args.get(at) {
        if !KNOWN_FLAGS.contains(&arg.as_str()) {
            return Some(arg);
        }
        at += 1;
        if args.get(at).is_some_and(|value| !value.starts_with("--")) {
            at += 1;
        }
    }
    None
}

/// lens が rules 行から読む 2 つ: cap（byte・`gate.token_cap`）と model（`runner.model`）。manifest は
/// `--rules PATH` が在ればそれ・無ければ埋め込み（[`rules_of`]・runner と同じ読み口）。
///
/// cap の行が無い / 不発効 / 整数でない周は `pipe::cli::int_row` と同じ 3 理由で `Err`（[`int_row`]）。
/// model の行も同じ極性で、閉じた表に無い値も `Err`（[`runner_model`]）。cap を先に読む（cap の 3 理由の
/// 字面は不変）。
fn rows_of(args: &[String]) -> Result<(u64, Model), String> {
    let manifest = rules_of(args)?;
    let cap = int_row(&manifest, ROW_CAP)?;
    let model = runner_model(&manifest)?;
    Ok((cap, model))
}

/// `lens` を 1 回。diff は stdin から byte で読む。
pub fn dispatch(args: &[String]) -> Outcome {
    let parsed = (|| {
        if let Some(found) = unknown_arg(args) {
            return Err(format!("未知の引数 {found}"));
        }
        Ok::<_, String>((
            need(args, "--contract")?.to_owned(),
            need(args, "--worktree")?.to_owned(),
            need(args, "--permission-mode")?.to_owned(),
            flag(args, "--account-dir")?.map(str::to_owned),
            flag(args, "--claude")?.map(str::to_owned),
        ))
    })();
    let (contract, worktree, mode, account, claude) = match parsed {
        Ok(found) => found,
        Err(reason) => return Outcome::failed(RC_REFUSED, vec![format!("lens: {reason}"), usage()]),
    };
    // **読めない契約で claude を起こさない**。材料が無いまま問えば返るのは
    // INCONCLUSIVE だけで、払った 1 回分は捨て金になる。
    let contract_path = Path::new(&contract);
    let contract = match Contract::load(contract_path) {
        Ok(found) => found,
        Err(errors) => {
            let first = errors.first().map_or_else(String::new, |err| err.reason.clone());
            return Outcome::failed_line(RC_BROKEN, format!("lens: 契約を読めない: {first}"));
        }
    };
    // **裁定は契約の写しの隣から読む**。在るのに読めない周は claude を起こさない（裁定を落とした
    // 審査は判定が揺れる側＝fail-closed）。
    let rulings = match rulings_of(contract_path) {
        Ok(found) => found,
        Err(reason) => return Outcome::failed_line(RC_BROKEN, format!("lens: 裁定を読めない: {reason}")),
    };
    // **cap が解けない周も claude を起こさない**（上限なしで走らせない＝C6）。model も同じ極性（版の既定へ
    // 黙って倒れない）。
    let (cap, model) = match rows_of(args) {
        Ok(found) => found,
        Err(reason) => return Outcome::failed_line(RC_BROKEN, format!("lens: {reason}")),
    };
    let prompt = match prompt_of(contract_path, &state(&contract), &rulings, cap) {
        Ok(found) => found,
        Err(outcome) => return outcome,
    };
    ask(&Call {
        claude: claude.as_deref().unwrap_or(DEFAULT_CLAUDE),
        prompt: &prompt,
        permission_mode: &mode,
        // rules 行の model を**毎回**渡す（claude CLI の別名・runner と同じ行）。
        model: Some(model.alias()),
        plugin_dir: None,
        account_dir: account.as_deref(),
        // **便の worktree で起こす**（anchor の repo は渡さない）。判定に載る憲法は
        // base の checkout のものであり、anchor 側の未 commit な `CLAUDE.md` ではない。
        cwd: Some(Path::new(&worktree)),
        // 判定を 1 つ受け取るだけなので既定（text）で呼ぶ。stream-json にすると
        // 「最後の JSON 行」が claude の result record になり、判定が取れない。
        streaming: false,
        max_turns: None,
    })
}

/// 審査の材料と prompt（**どちらの審査かは契約の隣の材料で決まる**）。
///
/// 材料が無い周は従来の diff の審査: stdin の diff を byte で読み、cap を超えたら claude を呼ばず INCONCLUSIVE。
/// 材料が在る周は契約の審査: stdin は読まず、契約 + 節 + 要件の byte で cap を照合する（同じ極性）。片方だけ
/// 在る・読めない周は `Err(rc 2)`（材料を落として審査しない）。**1 走査で埋める**——重ねて replace すると、
/// 先に埋めた契約本文の中の `{diff}` / `{design}` まで展開され、外から来る text が prompt の構造へ触れられる
/// （runner と同じ理由・裁定も同じ走査）。
fn prompt_of(contract: &Path, stated: &str, rulings: &str, cap: u64) -> Result<String, Outcome> {
    let material = material_of(contract).map_err(|reason| Outcome::failed_line(RC_BROKEN, format!("lens: {reason}")))?;
    let over = |bytes: usize| u64::try_from(bytes).unwrap_or(u64::MAX) > cap;
    match material {
        None => {
            let diff = read_stdin_bytes();
            if over(diff.len()) {
                // **claude を呼ばずに**返す。呼ばないことが cap の意味である。
                return Err(Outcome::ok_line(inconclusive("diff exceeds cap")));
            }
            Ok(fill(
                TEMPLATE,
                &[("{contract}", stated), ("{rulings}", rulings), ("{diff}", &String::from_utf8_lossy(&diff))],
            ))
        }
        Some((design, requirements)) => {
            if over(stated.len().saturating_add(design.len()).saturating_add(requirements.len())) {
                return Err(Outcome::ok_line(inconclusive("contract material exceeds cap")));
            }
            Ok(fill(
                CONTRACT_TEMPLATE,
                &[("{contract}", stated), ("{design}", &design), ("{requirements}", &requirements)],
            ))
        }
    }
}

/// 契約の写しの隣の [`DESIGN_FILE`] / [`REQUIREMENTS_FILE`]（契約の審査の材料・`pipe::review` が置く）。
///
/// 2 つとも無ければ `None`（diff の審査）・2 つとも在れば本文の対・片方だけ在る周と在るのに読めない周は `Err`
/// （呼び手が claude を起こさず rc 2 で止まる＝材料を落とした審査は偽の判定を出す側）。
fn material_of(contract: &Path) -> Result<Option<(String, String)>, String> {
    let read = |name: &str| -> Result<Option<String>, String> {
        let path = contract.with_file_name(name);
        match std::fs::read_to_string(&path) {
            Ok(found) => Ok(Some(found)),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
            Err(err) => Err(format!("{}: {err}", path.display())),
        }
    };
    match (read(DESIGN_FILE)?, read(REQUIREMENTS_FILE)?) {
        (None, None) => Ok(None),
        (Some(design), Some(requirements)) => Ok(Some((design, requirements))),
        (Some(_), None) => Err(format!("契約の隣に {DESIGN_FILE} だけが在る（{REQUIREMENTS_FILE} が無い）")),
        (None, Some(_)) => Err(format!("契約の隣に {REQUIREMENTS_FILE} だけが在る（{DESIGN_FILE} が無い）")),
    }
}

/// 契約の写しの隣の [`RULINGS_FILE`] を読む（path の導出はこの 1 か所）。
///
/// 無ければ [`NO_RULINGS`] の 1 行（「裁定なし」を明示する）。在るのに読めない周（dir が置かれている・
/// UTF-8 でない・権限が無い）は `Err`＝呼び手が claude を起こさず rc 2 で止まる。
fn rulings_of(contract: &Path) -> Result<String, String> {
    let path = contract.with_file_name(RULINGS_FILE);
    match std::fs::read_to_string(&path) {
        Ok(found) => Ok(found),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(NO_RULINGS.to_owned()),
        Err(err) => Err(format!("{}: {err}", path.display())),
    }
}

/// 契約を prompt へ差し込む形に組む（goal / done / verify 各行 / write-set 各行）。
///
/// **契約 file を丸写ししない**。lens が要るのは「何を作る契約か」と「何で測るか」で、
/// owner や disposition は判定の材料にならない——渡すほど cap（NFR1）を食う。
fn state(contract: &Contract) -> String {
    let listed = |lines: &[String]| {
        lines
            .iter()
            .map(|line| format!("- {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "goal: {}\ndone: {}\nverify:\n{}\nwrite-set:\n{}",
        contract.goal,
        contract.done,
        listed(&contract.verify),
        listed(&contract.write_set),
    )
}

/// claude を呼び、出力の**最後の JSON 行**を stdout 1 行に写す。
///
/// 起動形は [`build`] が持つ——lens は `--allowedTools` を渡さない側だが、settings 由来の
/// allow 規則は権限の口を開けるので、settings を 1 つも読まない形（`--setting-sources` の
/// 空値 + `--strict-mcp-config`）は runner と同じく毎回効く（ADR-0011 §2.1）。
fn ask(call: &Call<'_>) -> Outcome {
    let (mut command, confinement) = build(call);
    let spawned = command.spawn();
    let mut child = match spawned {
        Ok(found) => found,
        Err(err) => return Outcome::failed_line(RC_BROKEN, format!("lens: claude を起動できない: {err}")),
    };
    feed(&mut child, call.prompt);
    let waited = child.wait_with_output();
    // **終端で scope を片付ける**（設計 gate-cost.md §4.4 errata・`s2-07l.234`）。stdout の 1 行は
    // 判定の面なので、結果は stderr の 1 行だけに出す。
    let mut outcome = read_verdict(waited);
    let scope = confine::release_scope(&confinement);
    outcome.err.extend(scope.map(|released| format!("lens: scope={}", released.as_str())));
    outcome
}

/// 終わった claude の出力から最後の JSON 行を読む。
fn read_verdict(waited: std::io::Result<std::process::Output>) -> Outcome {
    let out = match waited {
        Ok(found) => found,
        Err(err) => return Outcome::failed_line(RC_BROKEN, format!("lens: claude の出力を読めない: {err}")),
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let found = text
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with(JSON_HEAD));
    match found {
        // 読めない出力を握り潰さない。**判定に届かなかった**と名乗る。
        None => Outcome::ok_line(inconclusive("lens output has no json line")),
        Some(line) => Outcome::ok_line(line.trim().to_owned()),
    }
}

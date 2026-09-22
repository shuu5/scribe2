//! `<NAME> lens`（設計 §6・FR9 / NFR1）。契約を `--contract` の path で・diff を stdin で
//! 受け、判定の JSON 1 行を返す。
//!
//! **契約が無ければ claude を呼ばない**。lens に問うのは「diff が契約の求めるものを
//! 満たすか」であって、diff だけを渡して同じことを問うことはできない——実 lens は
//! 「契約が未提供で適合を判定できない」と正しく INCONCLUSIVE を返し、便はそこで止まる
//! （実測 2026-09-10・s2-07l.24 の実 5 便）。**材料の不足は前提違反として断る**（rc 1）。
//! 契約を prompt へ差し込んでから判定を問うのが lens の口である。
//!
//! **`--worktree` は必須である**。lens に憲法（生成 file `docs/constitution.md`）を載せる経路は
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
//! 行が解けない周は cap と同じ極性＝claude を呼ばず rc 2。**effort も同じ manifest の rules 行 `runner.effort`
//! から読み、毎回渡す**（`s2-07l.322`・読む順は cap → model → effort）。
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
//!
//! **Promised の行（設計 contract-source.md §33 行 ah）は約束の行の写しも隣の file で受ける**。契約の審査の周に
//! [`PROMISES_FILE`] が在れば `{promises}` の穴に見出しと kind の 3 語の限りと写しを埋め、無ければ穴は空文字＝約束の行を
//! 持たない行の雛形は 1 字も変わらない。在るのに読めない周は材料の欠けと同じく claude を呼ばず rc 2。cap の照合にも足す。

use super::runner::{has_top_level_key, is_result_record, result_usage, scope_line, top_level_string};
use super::{
    build, feed, fill, flag, need, read_stdin_bytes, rules_of, runner_effort, runner_model, Call, Effort, Format,
    DEFAULT_CLAUDE,
};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
use crate::fleet::json_lite;
use crate::fleet::select::Model;
use crate::fleet::Usage;
use crate::pipe::confine;
use crate::pipe::contract::Contract;
use crate::pipe::move_proof::RULINGS_FILE;
use crate::pipe::review::{DESIGN_FILE, FINDING_KINDS, PROMISES_FILE, REQUIREMENTS_FILE};
use crate::rules::int_row;
use std::io::{ErrorKind, Read};
use std::path::Path;
use std::process::{Child, ExitStatus, Output};
use std::time::Duration;

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
const KNOWN_FLAGS: [&str; 7] =
    ["--contract", "--worktree", "--permission-mode", "--rules", "--account-dir", "--claude", "--cgroup-root"];

/// claude の終了を待つ poll の間隔（各周で scope の `memory.peak` を 1 回読む・設計 gate-cost.md §13）。
/// async は使わない（C13.3）。
const POLL: Duration = Duration::from_secs(1);

/// 使い方の 1 行。
pub fn usage() -> String {
    format!(
        "usage: {} lens --contract F --worktree D --permission-mode M [--rules PATH] [--account-dir D] [--claude PATH] [--cgroup-root DIR] < diff",
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

/// lens が rules 行から読む 3 つ: cap（byte・`gate.token_cap`）と model（`runner.model`）と effort（`runner.effort`）。
/// manifest は `--rules PATH` が在ればそれ・無ければ埋め込み（[`rules_of`]・runner と同じ読み口）。
///
/// cap の行が無い / 不発効 / 整数でない周は `pipe::cli::int_row` と同じ 3 理由で `Err`（[`int_row`]）。
/// model と effort の行も同じ極性で、閉じた表に無い値も `Err`（[`runner_model`] / [`runner_effort`]）。順は
/// cap → model → effort（先に落ちた理由 1 つだけを出す＝cap と model の字面は不変）。
fn rows_of(args: &[String]) -> Result<(u64, Model, Effort), String> {
    let manifest = rules_of(args)?;
    let cap = int_row(&manifest, ROW_CAP)?;
    let model = runner_model(&manifest)?;
    let effort = runner_effort(&manifest)?;
    Ok((cap, model, effort))
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
            // cgroup の root（claude の scope の peak の置き場・設計 gate-cost.md §13）。省くと typed な既定
            // [`confine::CGROUP_ROOT`]・env は読まない（C2.2）。
            flag(args, "--cgroup-root")?.map(str::to_owned),
        ))
    })();
    let (contract, worktree, mode, account, claude, cgroup_root) = match parsed {
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
    let (cap, model, effort) = match rows_of(args) {
        Ok(found) => found,
        Err(reason) => return Outcome::failed_line(RC_BROKEN, format!("lens: {reason}")),
    };
    let prompt = match prompt_of(contract_path, &state(&contract), &rulings, cap) {
        Ok(found) => found,
        Err(outcome) => return outcome,
    };
    ask(
        &Call {
        claude: claude.as_deref().unwrap_or(DEFAULT_CLAUDE),
        prompt: &prompt,
        permission_mode: &mode,
        // rules 行の model と effort を**毎回**渡す（claude CLI の字面・runner と同じ 2 行）。
        model: Some(model.alias()),
        effort: Some(effort.alias()),
        plugin_dir: None,
        account_dir: account.as_deref(),
        // **便の worktree で起こす**（anchor の repo は渡さない）。判定に載る憲法は
        // base の checkout のものであり、anchor 側の未 commit な `CLAUDE.md` ではない。
        cwd: Some(Path::new(&worktree)),
        // 判定と消費の 6 値を 1 object の封筒で受ける（設計 gate-cost.md §26 形 (2)）。stream-json にすると
        // 「最後の JSON 行」が claude の result record になり、判定が record の中の文字列へ埋もれる。
        output: Format::Json,
        max_turns: None,
        },
        Path::new(cgroup_root.as_deref().unwrap_or(confine::CGROUP_ROOT)),
    )
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
            let promises = promises_of(contract)
                .map(|found| promise_block(&found))
                .map_err(|reason| Outcome::failed_line(RC_BROKEN, format!("lens: 約束の行を読めない: {reason}")))?;
            let bytes = stated.len().saturating_add(design.len()).saturating_add(requirements.len());
            if over(bytes.saturating_add(promises.len())) {
                return Err(Outcome::ok_line(inconclusive("contract material exceeds cap")));
            }
            Ok(fill(
                CONTRACT_TEMPLATE,
                &[
                    ("{contract}", stated),
                    ("{design}", &design),
                    ("{requirements}", &requirements),
                    ("{promises}", &promises),
                ],
            ))
        }
    }
}

/// 契約の写しの隣の [`PROMISES_FILE`]（Promised の行だけ `pipe::review` が置く）。無ければ空・在るのに読めない周は `Err`。
fn promises_of(contract: &Path) -> Result<String, String> {
    let path = contract.with_file_name(PROMISES_FILE);
    match std::fs::read_to_string(&path) {
        Ok(found) => Ok(found),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(String::new()),
        Err(err) => Err(format!("{}: {err}", path.display())),
    }
}

/// `{promises}` の穴の本文: 写しが空なら空文字（雛形は 1 字も変わらない）・在れば見出しと kind の 3 語の限りと写し。
fn promise_block(rows: &str) -> String {
    let rows = rows.trim_end();
    if rows.is_empty() {
        return String::new();
    }
    let words: Vec<String> =
        FINDING_KINDS.iter().filter(|kind| kind.promised()).map(|kind| format!("`{}`", kind.as_str())).collect();
    format!(
        "\n## 約束の行（Promised・n の順）\nこの行は約束の行を持つ。write-set・verify・done は器が約束の行から生成し、歯の置き場・契約の字面・設計の材料は器が受付で測り終えている。各約束の行の fixture と expect がその text を測れているかを読む。FAIL と INCONCLUSIVE の周の `kind` は次の 3 語のちょうど 1 つに限る（他の語の FAIL は INCONCLUSIVE に倒される）: {}。\n\n{rows}",
        words.join(" / ")
    )
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
///
/// **claude の scope の peak は走行中に sample する**（設計 gate-cost.md §13・`s2-07l.273`）: 終了を
/// [`POLL`] の `try_wait` で待ち、各周で `memory.peak` を 1 回読む（[`confine::Sampler`]）。終端で読む形は
/// 最後の process の終了で scope が消えた正常系を測れない。**poll の間も stdout を読み切る**——子の stdout は
/// pipe なので、誰も読まないと 64 KiB で子が書き待ちになり poll が永久に回る（[`drain`]）。
fn ask(call: &Call<'_>, cgroup_root: &Path) -> Outcome {
    let (mut command, confinement) = build(call);
    let spawned = command.spawn();
    let mut child = match spawned {
        Ok(found) => found,
        Err(err) => return Outcome::failed_line(RC_BROKEN, format!("lens: claude を起動できない: {err}")),
    };
    feed(&mut child, call.prompt);
    let mut sampler = confine::Sampler::of(&confinement, cgroup_root);
    let waited = drain(&mut child, |child| poll(child, &mut sampler));
    // **終端で scope を片付ける**（設計 gate-cost.md §4.4 errata・`s2-07l.234`）。stdout の 1 行は
    // 判定の面なので、結果は stderr の 1 行だけに出す。
    let mut outcome = read_verdict(waited);
    outcome.err.extend(scope_line("lens", &confinement, sampler.peak()));
    outcome
}

/// 子の stdout を**別 thread で読み切りながら** `wait` を回し、終端で join して `Output` に組む。
///
/// stdout を取り出せない周（`build` は必ず pipe にする）は空の stdout で `wait` だけ回す。thread が落ちた周
/// （読み手の panic）も空＝読めない出力は INCONCLUSIVE へ倒れる側。stderr は子が親のものを継承する（空）。
fn drain(child: &mut Child, wait: impl FnOnce(&mut Child) -> std::io::Result<ExitStatus>) -> std::io::Result<Output> {
    let reader = child.stdout.take().map(|mut out| {
        std::thread::spawn(move || {
            let mut buffer = Vec::new();
            let _ = out.read_to_end(&mut buffer);
            buffer
        })
    });
    let status = wait(child)?;
    let stdout = reader.and_then(|handle| handle.join().ok()).unwrap_or_default();
    Ok(Output { status, stdout, stderr: Vec::new() })
}

/// claude の終了を [`POLL`] で待つ。**周ごとに 1 回 sample する**（眠った後・起動の直後は scope が未だ無い）。
fn poll(child: &mut Child, sampler: &mut confine::Sampler<'_>) -> std::io::Result<ExitStatus> {
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        std::thread::sleep(POLL);
        sampler.sample();
    }
}

/// 終わった claude の出力から判定の JSON 行を読む（[`verdict_line`]）。
fn read_verdict(waited: std::io::Result<std::process::Output>) -> Outcome {
    let out = match waited {
        Ok(found) => found,
        Err(err) => return Outcome::failed_line(RC_BROKEN, format!("lens: claude の出力を読めない: {err}")),
    };
    match verdict_line(&String::from_utf8_lossy(&out.stdout)) {
        // 読めない出力を握り潰さない。**判定に届かなかった**と名乗る。
        None => Outcome::ok_line(inconclusive("lens output has no json line")),
        Some(line) => Outcome::ok_line(line),
    }
}

/// text の最後の JSON 行（`{` で始まる行・trim 済み）。
fn last_json_line(text: &str) -> Option<&str> {
    text.lines().rev().map(str::trim).find(|line| line.starts_with(JSON_HEAD))
}

/// stdout から判定の 1 行を組む（**読みの分岐は 1 つ**・設計 gate-cost.md §26 形 (2)）。
///
/// 最後の JSON 行が `type` = `result` の封筒（claude の json 出力）なら、その `result` の text の最後の JSON 行を判定に
/// 読み、封筒の消費の 6 値（[`result_usage`]）を判定 object へ足す（[`with_usage`]）。封筒でなければその行をそのまま
/// 判定に読む（従来の text の形・偽 lens の fixture は不変）。判定の JSON 行が無い周は `None`。
fn verdict_line(text: &str) -> Option<String> {
    let last = last_json_line(text)?;
    if !is_result_record(last) {
        return Some(last.to_owned());
    }
    let result = top_level_string(last, "result")?;
    let verdict = last_json_line(&result)?;
    Some(with_usage(verdict, result_usage(last).as_ref()))
}

/// 判定 object へ消費の 3 対（[`Usage::pairs`]・`usage` / `turns` / `wall_ms`）を足す。
///
/// 6 値が揃わない周（`None`）・判定が既にどれかの key を持つ周（二重にすると flat な読み手が形の壊れと読む）・`}` で
/// 閉じない周は判定を 1 字も変えない（判定の意味は動かさない・読み手は field の不在を「測れなかった」と読む・C10）。
fn with_usage(verdict: &str, usage: Option<&Usage>) -> String {
    let Some(found) = usage else {
        return verdict.to_owned();
    };
    let pairs = found.pairs();
    if pairs.iter().any(|(key, _)| has_top_level_key(verdict, key)) {
        return verdict.to_owned();
    }
    let Some(head) = verdict.strip_suffix('}').map(str::trim_end) else {
        return verdict.to_owned();
    };
    let added = json_lite::write_object(&pairs);
    let added = added.strip_prefix('{').and_then(|rest| rest.strip_suffix('}')).unwrap_or_default();
    let comma = if head.ends_with(JSON_HEAD) { "" } else { "," };
    format!("{head}{comma}{added}}}")
}

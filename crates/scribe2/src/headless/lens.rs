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
//! **憲法の file が載らない周も claude を呼ばない**（設計 gate-cost.md §47 行 ar）。diff の審査の周は cap の後・prompt の前に
//! `--worktree` の木の憲法の file を在る・無い・読めないの 3 値に測り（[`constitution_of`]・中身は読まない）、
//! 無い周と読めない周は置く物か直す物を名指す INCONCLUSIVE の 1 行を返す。在る周の prompt は 1 字も変わらない。
//! 憲法の file は既定の 1 本だが、`--worktree` の HEAD の宣言の任意 key `constitution` が列を名乗れば既定を置き換え、1 本ずつ測って
//! 頼みの文の path の句を差し替える（[`template_of`]・設計 gate-cost.md §48）。
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
//! **model も同じ manifest の rules 行から読み、claude に毎回渡す**（`s2-07l.297`・設計 pipeline.md §6 / §61）。
//! `--stage` の無い lens（契約の審査と gate の審査）は `lens.model`、`--stage prelens` の lens（事前審査の先撃ち）は
//! `pipe.precheck_lens_model` を読む（[`STAGE_PRELENS`]・他の値と値の欠けは未知の引数と同じ断り）。読み口は
//! [`super::rules_of`] / [`super::model_row`]（runner と共通）で、行が解けない周は cap と同じ極性＝claude を呼ばず rc 2。
//! **effort も同じ manifest の rules 行 `runner.effort` から読み、毎回渡す**（`s2-07l.322`・読む順は cap → model → effort）。
//! **turn の上限も rules 行 `lens.max_turns` から読み、`--stage` に依らず `--max-turns` で毎回渡す**（設計 pipeline.md §67・
//! 読む順は effort の後・行が無い / 不発効 / 整数でない / 0 の周は claude を呼ばず rc 2）。封筒の `subtype` が `error_max_turns` の周は
//! 判定を採らず INCONCLUSIVE（[`verdict_line`]）で、gate が同じ gate の中で 1 回撃ち直す。
//!
//! **裁定（便の質問と回答の対）は契約の写しの隣の [`RULINGS_FILE`] から読む**（`s2-07l.309`・
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
//!
//! **write-set の base の要約（設計 contract-source.md §40 行 ao）も隣の file で受ける**。契約の審査の周に [`BASE_FILE`] が
//! 在れば `{base}` の穴に埋め、無ければ穴は空文字＝雛形は 1 字も変わらない。在るのに読めない周は rc 2。cap は新しい閾値を
//! 作らない: 既存の 4 材料だけで越える周は従来どおり claude を呼ばず INCONCLUSIVE、要約を足すと越える周は要約の段だけを
//! 落として落とした項目の本数の 1 行を残す（[`base_block`]）。
//!
//! **write-set の外の材料（設計 contract-source.md §51 行 bc）も隣の file で受ける**。[`OUTSIDE_FILE`] が在れば雛形の末尾の
//! `{outside}` の穴に埋め、無ければ空文字＝雛形は 1 字も変わらない。在るのに読めない周は rc 2。残りは base の段を足した後で
//! 測り、名ごとに収める（[`outside_block`]・収まらない名は切り詰めの 1 行・落とした名は本数の 1 行）。既存の 4 材料だけで
//! 越える周の INCONCLUSIVE と base の段の落とし方は不変。
//!
//! **done の番号つき項目（設計 contract-source.md §64 行 bs）も隣の file で受ける**。[`ITEMS_FILE`] が在れば契約の本文の後ろに空行
//! 1 つを挟んで足し（穴は足さない）、cap の照合に byte を数える。在るのに読めない周は rc 2。無ければ prompt は 1 字も変わらない。

use super::runner::{
    has_top_level_key, is_result_record, result_subtype, result_usage, scope_line, top_level_string, ResultKind,
};
use super::{
    build, feed, fill, flag, model_row, need, read_stdin_bytes, rules_of, runner_effort, Call, Effort, Format,
    DEFAULT_CLAUDE, ROW_LENS_MODEL, ROW_PRELENS_MODEL,
};
use crate::cli_args::{refusal, ArgsError};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
use crate::fleet::json_lite;
use crate::fleet::select::Model;
use crate::fleet::Usage;
use crate::hook::vessel::digest::fnv1a_64;
use crate::pipe::confine;
use crate::pipe::contract::Contract;
use crate::pipe::declaration::{ConstitutionFiles, DECL_FILE};
use crate::pipe::move_proof::RULINGS_FILE;
use crate::pipe::review::{base_block, outside_block, BASE_FILE, DESIGN_FILE, FINDING_KINDS, OUTSIDE_FILE, PROMISES_FILE};
use crate::pipe::review::ITEMS_FILE;
use crate::pipe::review::REQUIREMENTS_FILE;
use crate::rules::int_row;
use crate::rules::manifest::Manifest;
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

/// claude に毎回渡す turn の上限を持つ rules 行（`--stage` に依らず 1 行・設計 pipeline.md §67）。
const ROW_TURNS: &str = "lens.max_turns";

/// lens が受ける flag の全部（この外は未知の引数として断る）。
const KNOWN_FLAGS: [&str; 9] = [
    "--contract",
    "--worktree",
    "--permission-mode",
    "--rules",
    "--account-dir",
    "--claude",
    "--cgroup-root",
    "--stage",
    PRINT_VERSION,
];

/// 版の 1 行だけを stdout に出す flag（値を取らない・claude を起こさず、`--contract` / `--worktree` を要らず読まない・設計 row-review.md §5）。
const PRINT_VERSION: &str = "--print-version";

/// 版の 1 行の見出しの語。
const VERSION_HEAD: &str = "lens-version";

/// 段の flag（値は [`STAGE_PRELENS`] だけ・設計 pipeline.md §61）。
const STAGE_FLAG: &str = "--stage";

/// lens が claude に**毎回**渡す permission mode（許可の問いを誰にも出さない mode・渡された値は使わない・設計 pipeline.md §64 形 1）。
const PERMISSION_MODE: &str = "dontAsk";

/// lens が claude に渡す道具の列（読みの道具だけ・`--tools` の値・設計 pipeline.md §64 形 1）。
const READ_TOOLS: &str = "Read,Grep,Glob";

/// 渡された permission mode が [`PERMISSION_MODE`] でなかった周に、契約の file の dir へ置く 1 語の記録の file 名。
const IGNORED_FILE: &str = "lens.ignored";

/// `--stage` が取る値の 1 つ目（事前審査の先撃ちの lens・`pipe::dispatch::prelens` が lens の行の末尾に足す）。
pub const STAGE_PRELENS: &str = "prelens";

/// `--stage` が取る値の 2 つ目（memo の審査の lens・`pipe::dispatch::memo_lens` が lens の行の末尾に足す・設計 dispatcher.md §41）。
/// `--contract` は契約でなく memo の材料の file を指し、diff も契約の隣の材料も読まない。
pub const STAGE_MEMO: &str = "memo";

/// memo の審査の prompt の文面（tracked な template・穴は `{memo}` だけ・絶対 path も口座名も含まない）。
const MEMO_TEMPLATE: &str = include_str!("lens-memo.txt");

/// claude の終了を待つ poll の間隔（各周で scope の `memory.peak` を 1 回読む・設計 gate-cost.md §13）。
/// async は使わない（C13.3）。
const POLL: Duration = Duration::from_secs(1);

/// 使い方の 1 行。
pub fn usage() -> String {
    format!(
        "usage: {} lens --contract F --worktree D [--permission-mode M] [--rules PATH] [--account-dir D] [--claude PATH] [--cgroup-root DIR] [--stage prelens|memo] [--print-version] < diff",
        crate::name::NAME
    )
}

/// 封筒の `subtype` が `error_max_turns` の周の evidence（行の id を名指す・gate の撃ち直しの理由に載る）。
const TURNS_EVIDENCE: &str = "lens が turn の上限（lens.max_turns）で終わった";

/// 雛形 [`TEMPLATE`] にちょうど 1 回在る憲法の path の句（宣言 `constitution` が列を名乗る周に「（木の file `<path>`・…）」へ差し替える・
/// 雛形の字の写しで path としては読まない・設計 gate-cost.md §48 形 5）。
const CONSTITUTION_PHRASE: &str = "（起動 cwd の生成 file `docs/constitution.md`）";

/// 判定に届かなかった 1 行を組む。
fn inconclusive(reason: &str) -> String {
    format!(r#"{{"verdict":"INCONCLUSIVE","evidence":"{reason}"}}"#)
}

/// `--worktree` の木の憲法の file 1 本の 3 値（中身は読まない）。
enum Constitution {
    /// symlink を辿って開けた先が通常の file。
    Present,
    /// その path の項目そのものが無い（`docs` の dir が無い周も含む）。
    Absent,
    /// それ以外（dir・辿った先の無い symlink・権限で開けない・`docs` が file）。
    Unreadable,
}

/// 起こす前に `worktree` の木の `file`（repo 相対）を測る。lens の cwd と契約の dir は読まない。
/// 開く前に辿った先の種別を見る（FIFO を開いて待たない）。
fn constitution_of(worktree: &Path, file: &str) -> Constitution {
    let path = worktree.join(file);
    match std::fs::symlink_metadata(&path) {
        Err(err) if err.kind() == ErrorKind::NotFound => return Constitution::Absent,
        Err(_) => return Constitution::Unreadable,
        Ok(_) => {}
    }
    match std::fs::metadata(&path) {
        Ok(found) if found.is_file() && std::fs::File::open(&path).is_ok() => Constitution::Present,
        _ => Constitution::Unreadable,
    }
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
        if arg != PRINT_VERSION && args.get(at).is_some_and(|value| !value.starts_with("--")) {
            at += 1;
        }
    }
    None
}

/// lens が model を読む rules 行（`--stage` の値で選ぶ・設計 pipeline.md §61）: 無ければ `lens.model`・[`STAGE_PRELENS`] は
/// `pipe.precheck_lens_model`・[`STAGE_MEMO`] は `lens.model`。他の値は [`ArgsError::Unknown`]（字面 `--stage <値>`）・値の欠けは [`ArgsError::Missing`]＝
/// 未知の引数と同じ断り（[`refusal`]）で claude を呼ばない。
fn model_row_id(args: &[String]) -> Result<&'static str, ArgsError> {
    let Some(at) = args.iter().position(|arg| arg == STAGE_FLAG) else {
        return Ok(ROW_LENS_MODEL);
    };
    match args.get(at + 1).map(String::as_str) {
        Some(STAGE_PRELENS) => Ok(ROW_PRELENS_MODEL),
        Some(STAGE_MEMO) => Ok(ROW_LENS_MODEL),
        Some(value) if !value.starts_with("--") => Err(ArgsError::Unknown(format!("{STAGE_FLAG} {value}"))),
        _ => Err(ArgsError::Missing(STAGE_FLAG.to_owned())),
    }
}

/// lens が rules 行から読む 4 つ: cap（byte・`gate.token_cap`）と model（`row`＝[`model_row_id`]）と effort（`runner.effort`）と
/// turn の上限（`lens.max_turns`・`--stage` に依らず 1 つ・設計 pipeline.md §67）。
/// manifest は `--rules PATH` が在ればそれ・無ければ埋め込み（[`rules_of`]・runner と同じ読み口）。
///
/// cap の行が無い / 不発効 / 整数でない周は `pipe::cli::int_row` と同じ 3 理由で `Err`（[`int_row`]）。
/// model と effort の行も同じ極性で、閉じた表に無い値も `Err`（[`model_row`] / [`runner_effort`]）。turn の上限は値 0
/// （上限にならない）と `u32` に収まらない値も `Err`（[`turns_row`]）。順は
/// cap → model → effort → turn の上限（先に落ちた理由 1 つだけを出す＝cap と model と effort の字面は不変）。
fn rows_of(args: &[String], row: &str) -> Result<(u64, Model, Effort, u32), String> {
    let manifest = rules_of(args)?;
    let cap = int_row(&manifest, ROW_CAP)?;
    let model = model_row(&manifest, row)?;
    let effort = runner_effort(&manifest)?;
    let turns = turns_row(&manifest)?;
    Ok((cap, model, effort, turns))
}

/// rules 行 [`ROW_TURNS`] の値（`--max-turns` に渡す整数）。0 は上限にならないので断る。
fn turns_row(manifest: &Manifest) -> Result<u32, String> {
    match u32::try_from(int_row(manifest, ROW_TURNS)?) {
        Ok(found) if found > 0 => Ok(found),
        _ => Err(format!("{ROW_TURNS} の値が 1 以上 {} 以下の整数でない", u32::MAX)),
    }
}

/// `lens` を 1 回。diff は stdin から byte で読む。
pub fn dispatch(args: &[String]) -> Outcome {
    let row = match model_row_id(args) {
        Ok(found) => found,
        Err(error) => return refusal("lens", &error, usage()),
    };
    if args.iter().any(|arg| arg == PRINT_VERSION) {
        return version(args, row);
    }
    let parsed = (|| {
        if let Some(found) = unknown_arg(args) {
            return Err(format!("未知の引数 {found}"));
        }
        Ok::<_, String>((
            need(args, "--contract")?.to_owned(),
            need(args, "--worktree")?.to_owned(),
            // 任意の flag（受けても値は使わない・古い雛形が焼いた値は [`note_ignored`] が 1 語の記録に残す）。
            flag(args, "--permission-mode")?.map(str::to_owned),
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
    // memo の審査は契約を持たない（`--contract` は memo の材料の file）＝契約の読みも裁定も diff も通らない。
    if args.windows(2).any(|pair| pair.first().is_some_and(|flag| flag == STAGE_FLAG) && pair.get(1).is_some_and(|value| value == STAGE_MEMO)) {
        return memo(args, row, (&contract, &worktree), (claude.as_deref(), account.as_deref(), cgroup_root.as_deref()));
    }
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
    let (cap, model, effort, turns) = match rows_of(args, row) {
        Ok(found) => found,
        Err(reason) => return Outcome::failed_line(RC_BROKEN, format!("lens: {reason}")),
    };
    note_ignored(contract_path, mode.as_deref());
    let prompt = match prompt_of(contract_path, &state(&contract), &rulings, (cap, Path::new(&worktree))) {
        Ok(found) => found,
        Err(outcome) => return outcome,
    };
    ask(
        &call_of(&prompt, (model, effort, turns), claude.as_deref(), account.as_deref(), &worktree),
        Path::new(cgroup_root.as_deref().unwrap_or(confine::CGROUP_ROOT)),
    )
}

/// `--print-version` の 1 回（設計 row-review.md §5 行 h）: claude を起こさず、lens の子が claude を起こす時と同じ組み立て
/// （[`rows_of`] の cap と [`call_of`] が組む [`Call`] の欄のうち prompt の本文と path の値を除いた全部）と組み込みの雛形 3 本の
/// 本文の FNV-1a 64 を `key=value` で並べた 1 行を stdout に出す。`--contract` / `--worktree` は読まない。rules 行が解けない周は
/// claude を起こす周と同じ rc と断りの 1 行で、stdout は空。
fn version(args: &[String], row: &str) -> Outcome {
    if let Some(found) = unknown_arg(args) {
        return Outcome::failed(RC_REFUSED, vec![format!("lens: 未知の引数 {found}"), usage()]);
    }
    let (cap, model, effort, turns) = match rows_of(args, row) {
        Ok(found) => found,
        Err(reason) => return Outcome::failed_line(RC_BROKEN, format!("lens: {reason}")),
    };
    let call = call_of("", (model, effort, turns), None, None, "");
    let pairs = [
        ("model", call.model.unwrap_or_default().to_owned()),
        ("effort", call.effort.unwrap_or_default().to_owned()),
        ("cap", cap.to_string()),
        ("tools", call.tools.unwrap_or_default().to_owned()),
        ("permission", call.permission_mode.to_owned()),
        ("output", call.output.flag().unwrap_or("text").to_owned()),
        ("turns", call.max_turns.map_or_else(String::new, |found| found.to_string())),
        ("lens.txt", fnv1a_64(TEMPLATE.as_bytes())),
        ("lens-contract.txt", fnv1a_64(CONTRACT_TEMPLATE.as_bytes())),
        ("lens-memo.txt", fnv1a_64(MEMO_TEMPLATE.as_bytes())),
    ];
    let body: Vec<String> = pairs.iter().map(|(key, value)| format!("{key}={value}")).collect();
    Outcome::ok_line(format!("{VERSION_HEAD} {}", body.join(" ")))
}

/// claude を 1 回起こす材料（契約の審査・diff の審査・memo の審査が同じ 1 本で組む）。
fn call_of<'a>(
    prompt: &'a str,
    (model, effort, turns): (Model, Effort, u32),
    claude: Option<&'a str>,
    account: Option<&'a str>,
    worktree: &'a str,
) -> Call<'a> {
    Call {
        claude: claude.unwrap_or(DEFAULT_CLAUDE),
        prompt,
        // permission mode は渡された値に依らず dontAsk を**毎回**明示し、道具は読みの 3 つだけを渡す（設計 pipeline.md §64 形 1）。
        permission_mode: PERMISSION_MODE,
        // rules 行の model と effort を**毎回**渡す（claude CLI の字面・runner と同じ 2 行）。
        model: Some(model.alias()),
        effort: Some(effort.alias()),
        tools: Some(READ_TOOLS),
        plugin_dir: None,
        account_dir: account,
        // **便の worktree で起こす**（anchor の repo は渡さない）。判定に載る憲法は
        // base の checkout のものであり、anchor 側の未 commit な `CLAUDE.md` ではない。
        cwd: Some(Path::new(worktree)),
        // 判定と消費の 6 値を 1 object の封筒で受ける（設計 gate-cost.md §26 形 (2)）。stream-json にすると
        // 「最後の JSON 行」が claude の result record になり、判定が record の中の文字列へ埋もれる。
        output: Format::Json,
        // turn の上限は rules 行 `lens.max_turns` の値を**毎回**渡す（段に依らず・設計 pipeline.md §67）。
        max_turns: Some(turns),
    }
}

/// `--stage memo` の 1 回（設計 dispatcher.md §41 形 5）: `--contract` の file を memo の材料として読み、雛形の `{memo}` に
/// 埋めて問う。材料が読めない周と rules 行が解けない周は claude を起こさず rc 2。材料は cap（`gate.token_cap`・byte）で
/// 切る（既存の prompt の上限・文字の境界で落とす）。stdin は読まない。
fn memo(args: &[String], row: &str, (material, worktree): (&str, &str), (claude, account, cgroup_root): (Option<&str>, Option<&str>, Option<&str>)) -> Outcome {
    let text = match std::fs::read_to_string(material) {
        Ok(found) => found,
        Err(err) => return Outcome::failed_line(RC_BROKEN, format!("lens: memo の材料を読めない: {material}: {err}")),
    };
    let (cap, model, effort, turns) = match rows_of(args, row) {
        Ok(found) => found,
        Err(reason) => return Outcome::failed_line(RC_BROKEN, format!("lens: {reason}")),
    };
    let mut end = usize::try_from(cap).unwrap_or(usize::MAX).min(text.len());
    while !text.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    let prompt = fill(MEMO_TEMPLATE, &[("{memo}", text.get(..end).unwrap_or_default())]);
    ask(
        &call_of(&prompt, (model, effort, turns), claude, account, worktree),
        Path::new(cgroup_root.unwrap_or(confine::CGROUP_ROOT)),
    )
}

/// 渡された permission mode が [`PERMISSION_MODE`] でない周は、契約の file の dir に 1 語の記録 [`IGNORED_FILE`] を置く
/// （字は `ignored:` に渡された値を続けた 1 行）。dontAsk の周と flag の無い周は置かず、前の周の file が在れば消す
/// （どの雛形が古いかを便の記録の dir の 1 語で分かるようにする・stderr は 3 つの呼び手が捨てるので使わない）。
/// 書けない・消せない周は判定を動かさない（記録は判定の材料でない）。
fn note_ignored(contract: &Path, mode: Option<&str>) {
    let path = contract.with_file_name(IGNORED_FILE);
    match mode.filter(|value| *value != PERMISSION_MODE) {
        Some(value) => {
            let _ = std::fs::write(&path, format!("ignored:{value}\n"));
        }
        None => {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// 審査の材料と prompt（**どちらの審査かは契約の隣の材料で決まる**）。
///
/// 材料が無い周は従来の diff の審査: stdin の diff を byte で読み、cap を超えたら claude を呼ばず INCONCLUSIVE。
/// 材料が在る周は契約の審査: stdin は読まず、契約 + 節 + 要件の byte で cap を照合する（同じ極性）。片方だけ
/// 在る・読めない周は `Err(rc 2)`（材料を落として審査しない）。**1 走査で埋める**——重ねて replace すると、
/// 先に埋めた契約本文の中の `{diff}` / `{design}` まで展開され、外から来る text が prompt の構造へ触れられる
/// （runner と同じ理由・裁定も同じ走査）。
fn prompt_of(contract: &Path, stated: &str, rulings: &str, (cap, worktree): (u64, &Path)) -> Result<String, Outcome> {
    let material = material_of(contract).map_err(|reason| Outcome::failed_line(RC_BROKEN, format!("lens: {reason}")))?;
    let over = |bytes: usize| u64::try_from(bytes).unwrap_or(u64::MAX) > cap;
    match material {
        None => {
            let diff = read_stdin_bytes();
            if over(diff.len()) {
                // **claude を呼ばずに**返す。呼ばないことが cap の意味である。
                return Err(Outcome::ok_line(inconclusive("diff exceeds cap")));
            }
            // **憲法が載らない周も claude を呼ばない**（観点 `constitution` を 0 と数えさせない・設計 gate-cost.md §47 行 ar）。
            // 測るのは diff の審査の周の cap の後だけ（契約の審査と memo の段は憲法を載せる面でない）。
            let template = template_of(worktree)?;
            Ok(fill(
                &template,
                &[("{contract}", stated), ("{rulings}", rulings), ("{diff}", &String::from_utf8_lossy(&diff))],
            ))
        }
        Some((design, requirements)) => {
            let promises = beside(contract, PROMISES_FILE)
                .map(|found| promise_block(&found))
                .map_err(|reason| Outcome::failed_line(RC_BROKEN, format!("lens: 約束の行を読めない: {reason}")))?;
            let summary = beside(contract, BASE_FILE)
                .map_err(|reason| Outcome::failed_line(RC_BROKEN, format!("lens: base の要約を読めない: {reason}")))?;
            let copy = beside(contract, OUTSIDE_FILE)
                .map_err(|reason| Outcome::failed_line(RC_BROKEN, format!("lens: 外の材料を読めない: {reason}")))?;
            let items = beside(contract, ITEMS_FILE)
                .map_err(|reason| Outcome::failed_line(RC_BROKEN, format!("lens: done の項目を読めない: {reason}")))?;
            let bytes = stated.len().saturating_add(design.len()).saturating_add(requirements.len());
            let bytes = bytes.saturating_add(promises.len()).saturating_add(items.len());
            if over(bytes) {
                return Err(Outcome::ok_line(inconclusive("contract material exceeds cap")));
            }
            // 要約は**最後に**足す: 既存の 4 材料の残りに収まらなければ段ごと落とす（新しい閾値を作らない）。
            let room = cap.saturating_sub(u64::try_from(bytes).unwrap_or(u64::MAX));
            let base = base_block(&summary, room);
            // 外の材料は base の段を足した**後**の残りで名ごとに収める（§51 形 4）。
            let outside = outside_block(&copy, room.saturating_sub(u64::try_from(base.len()).unwrap_or(u64::MAX)));
            // done の項目は契約の本文の後ろに空行 1 つを挟んで足す（items.txt が無い周は 1 字も変わらない・§64 形 2）。
            let stated = if items.is_empty() { stated.to_owned() } else { format!("{stated}\n\n{}", items.trim_end_matches('\n')) };
            Ok(fill(
                CONTRACT_TEMPLATE,
                &[
                    ("{contract}", &stated),
                    ("{design}", &design),
                    ("{requirements}", &requirements),
                    ("{promises}", &promises),
                    ("{base}", &base),
                    ("{outside}", &outside),
                ],
            ))
        }
    }
}

/// 憲法の測りと頼みの文の雛形（設計 gate-cost.md §48 形 4・5）: `worktree` の HEAD の宣言（作業ツリーでなく）が名乗る列を [`ConstitutionFiles`] で読み、
/// 読めない宣言と最初に在るでない file は claude を起こさない INCONCLUSIVE の 1 行（`Err`）。全部在れば、Fixed は [`TEMPLATE`] のまま・
/// Declared は [`CONSTITUTION_PHRASE`] を木の file の列（書いた順・`・` 区切り）に差し替えた雛形を返す。
fn template_of(worktree: &Path) -> Result<String, Outcome> {
    let places = ConstitutionFiles::at(worktree, "HEAD");
    let Some(files) = places.files() else {
        return Err(Outcome::ok_line(inconclusive(&format!("constitution declaration unreadable: {DECL_FILE}"))));
    };
    for file in &files {
        let reason = match constitution_of(worktree, file) {
            Constitution::Present => continue,
            Constitution::Absent => format!("constitution file absent: {file} (place the constitution or a pointer to it)"),
            Constitution::Unreadable => format!("constitution file unreadable: {file}"),
        };
        return Err(Outcome::ok_line(inconclusive(&reason)));
    }
    if !matches!(places, ConstitutionFiles::Declared { .. }) {
        return Ok(TEMPLATE.to_owned());
    }
    let named: Vec<String> = files.iter().map(|file| format!("`{file}`")).collect();
    Ok(TEMPLATE.replacen(CONSTITUTION_PHRASE, &format!("（木の file {}）", named.join("・")), 1))
}

/// 契約の写しの隣の任意の材料（[`PROMISES_FILE`]〔Promised の行だけ `pipe::review` が置く〕と [`BASE_FILE`] と [`OUTSIDE_FILE`] と
/// [`ITEMS_FILE`]〔done の項目・契約の本文の後ろに足す・§64〕）。無ければ
/// 空・在るのに読めない周は `Err`。
fn beside(contract: &Path, name: &str) -> Result<String, String> {
    let path = contract.with_file_name(name);
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
/// 封筒の `subtype` が `error_max_turns` の周は `result` を読まず、[`TURNS_EVIDENCE`] の INCONCLUSIVE に消費の 3 対を足す。
fn verdict_line(text: &str) -> Option<String> {
    let last = last_json_line(text)?;
    if !is_result_record(last) {
        return Some(last.to_owned());
    }
    // 上限で終わった周は `result` を読まない（途中の判定が PASS でも採らない・AC3・設計 pipeline.md §67 形 4）。
    if result_subtype(last) == Some(ResultKind::ErrorMaxTurns) {
        return Some(with_usage(&inconclusive(TURNS_EVIDENCE), result_usage(last).as_ref()));
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

#[cfg(test)]
mod tests {
    use super::CONTRACT_TEMPLATE;
    use crate::cli_outcome::Outcome;
    use crate::pipe::review::{BASE_FILE, DESIGN_FILE, ITEMS_FILE, OUTSIDE_FILE, REQUIREMENTS_FILE};
    use std::path::{Path, PathBuf};

    /// (h) 雛形が差し替えの句をちょうど 1 回持つ（差し替えは `replacen` の 1 回・句が雛形から消えると Declared の周の差し替えが空振りする）。
    #[test]
    fn lens_constitution_phrase_is_in_the_template_exactly_once() {
        assert_eq!(super::TEMPLATE.matches(super::CONSTITUTION_PHRASE).count(), 1);
    }

    /// 契約の審査の周（材料が在る周）だけを撃つ歯なので worktree は測られない（憲法の測りは diff の審査の周だけ）。
    fn prompt_of(contract: &Path, stated: &str, rulings: &str, cap: u64) -> Result<String, Outcome> {
        super::prompt_of(contract, stated, rulings, (cap, Path::new(".")))
    }

    /// 契約の写しの隣に設計の節と要件（と `base` が在れば base の要約）を置いた tmp dir と写しの path。
    fn materials(name: &str, base: Option<&str>) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("headless-lens-base-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(DESIGN_FILE), "節の本文\n");
        let _ = std::fs::write(dir.join(REQUIREMENTS_FILE), "FR1: 要件の本文\n");
        if let Some(body) = base {
            let _ = std::fs::write(dir.join(BASE_FILE), body);
        }
        (dir.join("contract.toml"), dir)
    }

    /// 要約の写し（契約・材料・雛形のどれにも現れない字面の項目 2 本）。
    const SUMMARY: &str = "- src/zq.rs: 行数 全体 7 / 本体 5\n  宣言: fn zq_one\n  歯: zq_tooth\n- docs/zq.md: 行数 全体 2 / 本体 2\n";

    /// 契約の本文（穴の字面 `{base}` を持つ＝1 走査なら展開されない）。
    const STATED: &str = "goal: 穴の字面 {base} を持つ契約\ndone: d";

    /// 写しが在れば `{base}` の穴が要約の本文で埋まり、無ければ雛形は 1 字も変わらず（穴は空文字・prompt の末尾は要件の
    /// 本文のまま）、契約の本文の中の `{base}` はどちらの周も展開されない（1 走査）。
    #[test]
    fn headless_lens_base_fills_the_hole_only_when_the_copy_exists_in_one_pass() {
        assert!(CONTRACT_TEMPLATE.contains("{requirements}{promises}{base}{outside}\n"), "穴は雛形の末尾に並ぶ");
        let (contract, dir) = materials("with", Some(SUMMARY));
        let filled = prompt_of(&contract, STATED, "（裁定なし）", u64::MAX).unwrap_or_default();
        assert!(filled.contains(SUMMARY.trim_end()), "要約の本文が埋まる: {filled}");
        assert!(filled.contains("\n## write-set の base の要約"), "見出しを持つ: {filled}");
        assert!(filled.contains("穴の字面 {base} を持つ契約"), "契約の中の穴は展開されない: {filled}");
        assert_eq!(filled.matches("src/zq.rs").count(), 1, "要約は 1 回だけ埋まる");
        let _ = std::fs::remove_dir_all(&dir);
        let (contract, dir) = materials("without", None);
        let bare = prompt_of(&contract, STATED, "（裁定なし）", u64::MAX).unwrap_or_default();
        assert!(bare.ends_with("## 契約が満たす要件\nFR1: 要件の本文\n\n"), "穴は空文字＝末尾は要件の本文: {bare:?}");
        assert!(!bare.contains("base の要約") && !bare.contains("src/zq.rs"), "{bare}");
        assert!(bare.contains("穴の字面 {base} を持つ契約"), "契約の中の穴は写しが無くても展開されない");
        let expected = CONTRACT_TEMPLATE
            .replacen("{contract}", STATED, 1)
            .replacen("{design}", "節の本文\n", 1)
            .replacen("{requirements}{promises}{base}{outside}", "FR1: 要件の本文\n", 1);
        assert_eq!(bare, expected, "写しが無い周の prompt は穴を足す前の雛形と同じ");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 外の材料の写し（契約・材料・雛形のどれにも現れない字面の塊 2 本・本文に穴の字面 `{outside}` / `{base}` を持つ）。
    const OUTSIDE: &str = "- ZqOuter: src/zq_outer.rs:3\n  pub struct ZqOuter {\n      pub zq_field: u8,\n  }\n- data/zq.json: 行数 2 / byte 20\n  穴の字面 {outside} と {base}\n";

    /// [`materials`] の dir に外の材料の写しを足す。
    fn with_outside(name: &str, base: Option<&str>, outside: &str) -> (PathBuf, PathBuf) {
        let (contract, dir) = materials(name, base);
        let _ = std::fs::write(dir.join(OUTSIDE_FILE), outside);
        (contract, dir)
    }

    /// 写しが在れば雛形の末尾の `{outside}` の穴が見出しと塊で埋まり（base の要約の後ろ）、無ければ雛形は 1 字も変わらず、
    /// 契約の本文と写しの本文の中の穴の字面は展開されない（1 走査）。
    #[test]
    fn headless_lens_outside_fills_the_last_hole_only_when_the_copy_exists() {
        let stated = "goal: 穴の字面 {outside} を持つ契約\ndone: d";
        let (contract, dir) = with_outside("outside-with", Some(SUMMARY), OUTSIDE);
        let filled = prompt_of(&contract, stated, "（裁定なし）", u64::MAX).unwrap_or_default();
        assert!(filled.ends_with(&format!("{}\n", OUTSIDE.trim_end())), "塊が末尾に埋まる: {filled}");
        assert!(filled.contains("\n## write-set の外の材料"), "見出しを持つ: {filled}");
        let (base_at, outside_at) = (filled.find("src/zq.rs"), filled.find("ZqOuter"));
        assert!(base_at.is_some() && base_at < outside_at, "base の要約の後ろ: {filled}");
        assert!(filled.contains("穴の字面 {outside} を持つ契約") && filled.contains("穴の字面 {outside} と {base}"), "展開しない: {filled}");
        assert_eq!(filled.matches("ZqOuter {").count(), 1, "1 回だけ埋まる");
        let _ = std::fs::remove_dir_all(&dir);
        let (contract, dir) = materials("outside-without", Some(SUMMARY));
        let bare = prompt_of(&contract, stated, "（裁定なし）", u64::MAX).unwrap_or_default();
        assert!(!bare.contains("write-set の外の材料") && !bare.contains("ZqOuter"), "{bare}");
        let with_base = CONTRACT_TEMPLATE.replacen("{outside}", "", 1);
        assert!(bare.ends_with(&format!("{}\n", SUMMARY.trim_end())), "穴は空文字＝末尾は base の要約: {bare:?}");
        assert!(with_base.ends_with("{base}\n"), "穴を空にした雛形は base の穴で終わる");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 残りは base の段を足した**後**で測る: base の要約と外の材料の全部がちょうど収まる cap では両方が全部入り、base の段の
    /// byte を数えずに外の材料だけが収まる cap では外の材料は名ごとの切り詰めの行になる（base の段は全部のまま）。既存の 4 材料
    /// だけで越える周は外の材料が在っても INCONCLUSIVE のまま。
    // flip-check: retroactive s2-07l.736.3
    #[test]
    fn headless_lens_outside_room_is_measured_after_the_base_stage() {
        let base = u64::try_from(crate::pipe::review::base_block(SUMMARY, u64::MAX).len()).unwrap_or(u64::MAX);
        let long = format!("{OUTSIDE}  {}\n", "z".repeat(usize::try_from(base).unwrap_or(0).saturating_add(600)));
        let (contract, dir) = with_outside("outside-room", Some(SUMMARY), &long);
        let four = u64::try_from([STATED, "節の本文\n", "FR1: 要件の本文\n"].iter().map(|text| text.len()).sum::<usize>()).unwrap_or(u64::MAX);
        let outside = u64::try_from(super::outside_block(&long, u64::MAX).len()).unwrap_or(u64::MAX);
        let both = prompt_of(&contract, STATED, "（裁定なし）", four + base + outside).unwrap_or_default();
        assert!(both.contains(SUMMARY.trim_end()) && both.contains(&"z".repeat(600)), "両方が全部入る: {both}");
        let squeezed = prompt_of(&contract, STATED, "（裁定なし）", four + outside).unwrap_or_default();
        assert!(squeezed.contains(SUMMARY.trim_end()), "base の段は全部のまま: {squeezed}");
        assert!(!squeezed.contains(&"z".repeat(600)) && squeezed.contains("切り詰めた"), "外の材料は切り詰め: {squeezed}");
        let over = prompt_of(&contract, STATED, "（裁定なし）", four.saturating_sub(1)).map_err(|outcome| outcome.out);
        assert_eq!(over, Err(vec![r#"{"verdict":"INCONCLUSIVE","evidence":"contract material exceeds cap"}"#.to_owned()]));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// done の項目の写し（契約・材料・雛形のどれにも現れない字面 `zi`）。
    const ITEMS: &str = "## done の項目（2 個・番号つき）\n表の形 zi\n(1) 甲 zi\n(2) 乙 zi\n";

    /// (b) 写しの隣に items.txt が在れば、prompt の契約の本文の直後（設計の節の前）に空行 1 つと items.txt の本文（末尾の改行を除く）
    /// が 1 回だけ入り、雛形の穴は増えない。cap は契約 + items + 設計 + 要件の byte ちょうどで prompt を組み、1 byte 少ないと
    /// INCONCLUSIVE。items.txt の名の dir が在る周は prompt を組まず rc 2。
    #[test]
    fn headless_lens_items_follow_the_contract_body_count_in_the_cap_and_refuse_when_unreadable() {
        let (contract, dir) = materials("items", None);
        let _ = std::fs::write(dir.join(ITEMS_FILE), ITEMS);
        let filled = prompt_of(&contract, STATED, "（裁定なし）", u64::MAX).unwrap_or_default();
        let joined = format!("{STATED}\n\n{}\n\n## 契約が実装する設計の節\n", ITEMS.trim_end());
        assert!(filled.contains(&joined), "契約の本文の直後に空行 1 つと項目: {filled}");
        assert_eq!(filled.matches("(1) 甲 zi").count(), 1, "1 回だけ入る");
        let total = [STATED, ITEMS, "節の本文\n", "FR1: 要件の本文\n"].iter().map(|text| text.len()).sum::<usize>();
        let total = u64::try_from(total).unwrap_or(u64::MAX);
        assert!(prompt_of(&contract, STATED, "（裁定なし）", total).is_ok(), "ちょうどは組む");
        let over = prompt_of(&contract, STATED, "（裁定なし）", total.saturating_sub(1)).map_err(|outcome| outcome.out);
        assert_eq!(over, Err(vec![r#"{"verdict":"INCONCLUSIVE","evidence":"contract material exceeds cap"}"#.to_owned()]));
        let _ = std::fs::remove_dir_all(&dir);
        let (contract, dir) = materials("items-dir", None);
        let _ = std::fs::create_dir_all(dir.join(ITEMS_FILE));
        let refused = prompt_of(&contract, STATED, "（裁定なし）", u64::MAX).map_err(|outcome| outcome.rc);
        assert_eq!(refused, Err(crate::cli_outcome::RC_BROKEN), "読めない材料は prompt を組まない");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// cap の極性: 要約を足すと越える周は要約の段だけが落ちて本数の 1 行が残り（claude を呼ぶ側＝prompt が返る）、既存の
    /// 4 材料だけで越える周は prompt を組まず INCONCLUSIVE のまま。
    #[test]
    fn headless_lens_base_over_cap_drops_the_stage_but_existing_materials_stay_inconclusive() {
        let (contract, dir) = materials("cap", Some(SUMMARY));
        let four = [STATED, "節の本文\n", "FR1: 要件の本文\n"].iter().map(|text| text.len()).sum::<usize>();
        let four = u64::try_from(four).unwrap_or(u64::MAX);
        let dropped = prompt_of(&contract, STATED, "（裁定なし）", four).unwrap_or_default();
        assert!(dropped.contains("段ごと落とした: 項目 2 本"), "本数の 1 行: {dropped}");
        assert!(!dropped.contains("src/zq.rs"), "要約の本文は落ちる: {dropped}");
        let over = prompt_of(&contract, STATED, "（裁定なし）", four.saturating_sub(1)).map_err(|outcome| outcome.out);
        assert_eq!(
            over,
            Err(vec![r#"{"verdict":"INCONCLUSIVE","evidence":"contract material exceeds cap"}"#.to_owned()]),
            "既存の 4 材料だけで越える周は prompt を組まない"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

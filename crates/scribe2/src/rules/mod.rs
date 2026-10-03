//! 規則の**種類**を閉じた型で持つ面（憲法 C1「規則はデータ」）。
//!
//! 種類は [`RuleKind`] の閉じた列挙で、値は manifest（`rules/manifest.toml`）が
//! 持つ。`#[non_exhaustive]` は付けない: variant を足したら [`RuleKind::shape`]
//! と [`RuleKind::as_str`] の網羅 `match` が compile error になる形を保つ。
//!
//! 値の型と種類の対応は [`RuleKind::shape`] ただ 1 箇所に置き、wildcard `_` を
//! 書かない（対応の追加漏れを compile 時に落とすため）。

pub mod cli;
pub mod device;
pub mod exclusion;
mod groups;
pub mod manifest;

use crate::case::{turn_of, Phase, Turn, PHASES};
use crate::fleet::select::{Model, MODELS};
use crate::headless::{Effort, EFFORTS};
use crate::hook::host_guard::{publish, Protected, PROTECTED};
use crate::pipe::contract::{class_element, ClassElement, CLASSES};
use crate::seat::role::{Capability, Role, ALL as ROLES, CAPABILITIES};
use manifest::{HostManifest, Manifest};
use std::path::{Path, PathBuf};

/// `lifecycle.age_h.<語>` の行 id の頭（[`RuleKind::LifecycleAgeH`]）。
pub const AGE_ID_PREFIX: &str = "lifecycle.age_h.";

/// host の面の file 名（`<state_dir>/host.toml`・設計 account-lifecycle.md §2・ADR-0026 §2.1）。
pub const HOST_MANIFEST: &str = "host.toml";

/// host の面の path。**`--state-dir` からだけ解く**（env を読まない・C2.2）。
pub fn host_manifest_path(state_dir: &Path) -> PathBuf {
    state_dir.join(HOST_MANIFEST)
}

/// tracked の面に、state dir が在れば host の面を合わせる（**呼び手が host の面を読む口はこの 1 か所**）。
///
/// state dir を持たない呼び手（hook / polarity 等）は `None` を渡す＝tracked の面だけ（従来どおり）。
pub fn with_state_dir(tracked: Manifest, state_dir: Option<&Path>) -> Result<Manifest, Vec<RuleError>> {
    match state_dir {
        Some(dir) => tracked.with_host(&host_manifest_path(dir)),
        None => Ok(tracked),
    }
}

/// `--rules PATH`（無ければ埋め込み）の tracked の面を読み、[`with_state_dir`] で host の面を合わせる。
pub fn read(rules: Option<&Path>, state_dir: Option<&Path>) -> Result<Manifest, Vec<RuleError>> {
    with_state_dir(rules.map_or_else(Manifest::embedded, Manifest::load)?, state_dir)
}

/// tracked の面の label 列に `<state_dir>/host.toml` の label を足す（[`with_state_dir`] と同じ規則・`seat tick` の口）。
pub fn declared_labels(tracked: &[String], state_dir: &Path) -> Result<Vec<String>, Vec<RuleError>> {
    HostManifest::read(&host_manifest_path(state_dir)).labels_over(tracked)
}

/// `<state_dir>/host.toml` が宣言する各群の**今の口座**の label（**便用の選定の除外**・設計 account-lifecycle.md §23 形 1・
/// §17 の約束 4 の改め）。**便用の除外を置き場から解く口はこの 1 本**（`select_for_run` の束と `fleet select` の口の両方）。
///
/// 群ごとに [`crate::hook::group::current_of`]（記録 > 種）の label を集める（2 群が同じ今の口座なら 1 つ）。候補の残りは
/// 便用の候補に残る。tracked の面は群を持てない（置いた周は未知の表として断る）ので、読むのは host の面だけである。面が
/// 無い周は空（0 群・除外を増やさない）、読めない周は欠陥の全件、記録が在るのに読めない群が 1 つでも在る周は
/// [`GroupedError::Record`]（候補の全部に読み替えない・1 つも返さない）。記録は 1 件も書かない。
pub fn grouped_accounts(state_dir: &Path) -> Result<std::collections::BTreeSet<String>, GroupedError> {
    let face = match HostManifest::read(&host_manifest_path(state_dir)) {
        HostManifest::Absent => return Ok(std::collections::BTreeSet::new()),
        HostManifest::Unreadable(errors) => return Err(GroupedError::Manifest(errors)),
        HostManifest::Present(face) => face,
    };
    let current = |group: &manifest::AccountGroup| {
        crate::hook::group::current_of(state_dir, group)
            .map(|found| found.label)
            .map_err(|error| GroupedError::Record(group.name().to_owned(), error))
    };
    face.groups().iter().map(current).collect()
}

/// `<state_dir>/host.toml` が宣言する park の区画の**置き場**（anchor）の集合（設計 account-lifecycle.md §36 形 1）。
///
/// 区画の置き場の席の row は口座を占めない＝便用と session 用の除外に数えない（区画の席の row を外す読み手は
/// [`crate::fleet::State::run_registered_accounts`] と `choose`）。**[`grouped_accounts`] の隣の同じ面の読み**で、面が無い周・
/// 区画の無い面は空、読めない周は [`GroupedError::Manifest`]。記録は読まない（区画は今の口座を持たない）。
pub fn park_anchors(state_dir: &Path) -> Result<std::collections::BTreeSet<String>, GroupedError> {
    match HostManifest::read(&host_manifest_path(state_dir)) {
        HostManifest::Absent => Ok(std::collections::BTreeSet::new()),
        HostManifest::Unreadable(errors) => Err(GroupedError::Manifest(errors)),
        HostManifest::Present(face) => Ok(face.park().map(|lot| lot.anchors().iter().cloned().collect()).unwrap_or_default()),
    }
}

/// 便用の除外を置き場から解けない周の断り（[`grouped_accounts`]・閉じた 2 値）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupedError {
    /// host の面が在るのに読めない（欠陥の全件・行番号付き）。
    Manifest(Vec<RuleError>),
    /// 群（名）の今の口座の記録が在るのに読めない（読めなさの型）。
    Record(String, crate::hook::group::RecordError),
}

impl std::fmt::Display for GroupedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Manifest(errors) => write!(f, "{}", errors.iter().map(ToString::to_string).collect::<Vec<_>>().join(" / ")),
            Self::Record(group, error) => {
                write!(f, "group={group} record={} — 群の今の口座の記録が在るのに読めない（除外を決められず選ばない）", error.as_str())
            }
        }
    }
}

/// 発効した行を 1 つ引く。無い / 不発効の周は理由つきで `Err`（行の無さを既定に倒さない・C1）。
fn enabled_row<'a>(manifest: &'a Manifest, id: &str) -> Result<&'a RuleRow, String> {
    let row = manifest.get(id).ok_or(format!("{id} が無い"))?;
    if !row.enabled {
        return Err(format!("{id} は不発効である"));
    }
    Ok(row)
}

/// 整数の行の値。行が無い / 不発効 / 整数でない周は 3 理由の `Err`（`pipe::cli::int_row` と同じ字面）。
///
/// 読み手は headless（lens の cap）。`pipe::cli` の同形は private で、pub にするには `pipe` 側へ手を入れる
/// ことになる（`s2-07l.272` の柵）——**行の読み手の正本はこちら**で、`pipe` 側は後続で寄せる。
pub fn int_row(manifest: &Manifest, id: &str) -> Result<u64, String> {
    match enabled_row(manifest, id)?.value {
        RuleValue::Int(found) => Ok(found),
        _ => Err(format!("{id} が整数でない")),
    }
}

/// 文字列の行の値。行が無い / 不発効 / 文字列でない周は 3 理由の `Err`（[`int_row`] と同じ極性）。
///
/// 読み手は headless（runner / lens の model の 3 行）と `pipe::ratelimit`（便用の選定の model）の **1 本**。
pub fn str_row<'a>(manifest: &'a Manifest, id: &str) -> Result<&'a str, String> {
    match &enabled_row(manifest, id)?.value {
        RuleValue::Str(found) => Ok(found),
        _ => Err(format!("{id} が文字列でない")),
    }
}

/// 列の行の値。行が無い / 不発効 / 列でない周は 3 理由の `Err`（[`int_row`] と同じ極性・読み手は管理 tick の梯子）。
pub fn list_row<'a>(manifest: &'a Manifest, id: &str) -> Result<&'a [String], String> {
    match &enabled_row(manifest, id)?.value {
        RuleValue::List(found) => Ok(found),
        _ => Err(format!("{id} が列でない")),
    }
}

/// 種類が要求する値の形。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueShape {
    /// 閾値（単位は variant の doc コメントが持つ）。
    Int,
    /// 識別子。
    Str,
    /// 散文で書かれた選定規則・検出線の定義。機械は `enabled` だけを読む。
    Policy,
    /// 文字列の列（TOML の string array）。**空は受けない**＝「規則が無い」を
    /// 空 array で表さない（書き間違いを黙って通すと allowlist が空のまま効く）。
    List,
}

/// 規則の種類。憲法 §3 の行と MVP の運用値に 1:1 で対応する。
///
/// variant を足すと [`RuleKind::as_str`] と [`RuleKind::shape`] の網羅 `match` が
/// compile error になるので、種類の追加は必ず手が入る。[`ALL`] の並びが宣言順から
/// ずれた形（並べ替え・重複・**中間**の欠番）は [`crate::order::is_declaration_order`]
/// を通す歯が捕まえる（ADR-0013 §2.2）。ただし **[`ALL`] への足し忘れは機械が検出しない**
/// ——列挙の母集団が `ALL` 自身なので、抜けた variant は parity test の母集団からも消える。
/// 唯一の例外は **manifest 行を伴う**追加で、行の kind は `ALL` を通して解決されるため
/// 未知の kind として `parse` できず落ちる（2026-09-11 実測）。**行を伴わない追加は
/// どの面も受けない**（同日実測: 368/368 が緑のまま）。variant を足したら `ALL` にも
/// 足すこと。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleKind {
    /// core crate の `src` 配下 `.rs` の総行数の上限（行）。
    CoreLines,
    /// `crates/*/src` 配下 `.rs` 1 file あたりの物理行数の上限（行）。
    ModuleLines,
    /// test 行 / src 行の比の上限（百分率・100 = 比 1.0）。
    TestSrcRatioPct,
    /// 関数 1 本の行数の上限（行）。
    FnLines,
    /// 関数 1 本の認知的複雑度の上限。
    FnComplexity,
    /// 関数 1 本の引数の数の上限。
    FnArgs,
    /// 行の数え方の幅（文字）。これを超える行は ceil(文字数 ÷ 幅) 行に数える（R-C4-1〜3 と上限の余地が同じ式）。
    LineWidth,
    /// 境界 crate の `src` の本体の総行数の上限（行・R-C4-5・設計 core-boundary.md §3 / §9・ADR-0062）。core の外へ判定を
    /// 押し出して core-lines から逃げる形を塞ぐ。読み手は xtask 側（core は値を消費しない）。
    BoundaryLines,
    /// 承認の受理面の identity。
    DialogueSurface,
    /// 成熟条件（停止・履歴として残す）。
    MaturityCondition,
    /// session 用の口座選定の閾値（使用率の百分率・未満の口座だけが候補）。便用の規則は閾値を持たない。
    AccountSelection,
    /// 変異生存率の検出線。
    MutationSurvivalLine,
    /// 直接依存の本数の予算（本）。
    DepBudget,
    /// 1 PR で足せる依存の本数（本）。
    DepPerPr,
    /// 依存追加時の増分 check 実測差の許容（ミリ秒）。
    CheckDeltaMs,
    /// compile の決定論的な形。
    CompileShape,
    /// compile 秒数の検出線。
    CompileSeconds,
    /// 1 周の review で回す lens の本数（本）。
    GateLensCount,
    /// 事前審査の先撃ちの lens を 1 周に起こす本数の上限（本・設計 dispatcher.md §27 形 1）。撃ち中の行を含めて数え、0 は撃たない。
    /// 任意の行で、読めない周は撃たず `[DISPATCH-PRECHECK]` の行の末尾に `prelens=unset` を足す。
    PipePrecheckLensPerRound,
    /// 1 周の gate の token 上限（token）。
    GateTokenCap,
    /// 便ごとの token 消費の検出線（token・憲法 C6.2 の R-C6-1・設計 gate-cost.md §43）。便の消費の event の 4 値の和が
    /// この値以上の便に `pipe show` が判定行を 1 行出す。読み手はその 1 か所で、便を断る読み手は持たない。
    RunTokenCeiling,
    /// lens が claude に毎回渡す turn の上限（turn・設計 pipeline.md §67）。値 0 は上限にならないので lens が断る。
    /// 読み手は lens の `rows_of` の 1 本で、行を読めない周は claude を呼ばず rc 2。
    LensMaxTurns,
    /// 上限の許可の対象の行の列（設計 limit-permit.md §18 約束 1）。値は行 id の列で、各要素は manifest の行の id で、その行の kind が
    /// [`RuleKind::has_permit_reader`] の true でなければ読み込みで断る。読み手は許可の口と gate（後の行）。
    PipePermitRows,
    /// 上限の許可の期限の上限（時間・設計 limit-permit.md §18 約束 1）。許可の口が発話の時刻からの期限の内側かを照らす（後の行）。
    PipePermitMaxH,
    /// hook 1 回の実行予算（ミリ秒）。
    HookBudgetMs,
    /// publish の配線が子を撃つ段の締め切り（ミリ秒・NFR5）。配線の timeout 未満に限る。
    HostGuardPublishDeadlineMs,
    /// publish の配線が子の出力を読む上限（byte・NFR5）。
    HostGuardPublishReadBytes,
    /// pipeline の停止猶予（ミリ秒）。
    StopGraceMs,
    /// fleet の lock 再取得間隔（ミリ秒）。
    LockRetryMs,
    /// fleet の lock を stale と見なす経過時間（ミリ秒）。
    LockStaleMs,
    /// hook の timeout（秒）。
    HookTimeoutS,
    /// **宣言が名乗れる上限**（ADR-0010 §2.2）。対象 repo の vessel 宣言
    /// `allowed-commands` はこの部分集合でなければ intake が便を起こさない。
    RunnerAllowedCommands,
    /// **禁じる語列**（ADR-0025 §2.1）。[`Self::RunnerAllowedCommands`] と対で読む**上限側の禁止**で、値は
    /// 空白区切りの語列の配列（先頭語が一致し残りの語をすべて含む command を hook の command guard と intake が
    /// 止める）。vessel 宣言は緩められない（C14）。
    RunnerDeniedCommands,
    /// **tracked な非 Rust 実行物の例外**（ADR-0009 §2.5）。分類器（shebang / 実行 bit /
    /// 拡張子）に当たる path のうち、この列に**完全一致**で載るものだけを `xtask check` が
    /// 通す。定義を緩める代わりに例外を 1 面へ集めるための行である。
    RepoNonRustExecAllow,
    /// 席の cycle が**作り直しと復元を確認する上限**（秒）。超えたら `clear-unconfirmed` /
    /// `restore-unconfirmed` で止まる。
    SeatCycleSettleS,
    /// 席の cycle が**確認を見に行く周期**（ミリ秒）。上限の内でこの刻みで証拠を読み直す。
    SeatCyclePollMs,
    /// 口座残量を聞きに行く子 process の待ち時間の上限（秒）。
    UsageTimeoutS,
    /// 選定の前計測の鮮度（秒・設計 account-autonomy.md §13）。最新の回が全部実測でその ts が
    /// `now − 値` より新しい口座は測り直さない。`fleet usage` の口は読まない（鮮度に関わらず全口座を測る）。
    UsageFreshS,
    /// 群の逼迫の 5 時間窓の閾値（使用率の百分率・設計 account-lifecycle.md §19 形 1）。最新の実測がこの値以上の口座を
    /// 逼迫と判じる。読み手は dispatch の 1 周の群の段と席の hook（同じ 1 本の読み手・[`crate::hook::group`]）。
    GroupPressure5hPct,
    /// 群の逼迫の 7 日窓の閾値（使用率の百分率・[`Self::GroupPressure5hPct`] と対で読む）。
    GroupPressure7dPct,
    /// 群の逼迫のモデル別 7 日窓の閾値（使用率の百分率・[`Self::GroupPressure5hPct`] と対で読む）。
    GroupPressureModelPct,
    /// land の追随が衝突した便を**起こし直す回数の上限**（回）。値 N = 最大 N 回起こし直す
    /// （N+1 回目の衝突で終端する）。
    FollowRetries,
    /// 終わりの門（runner の終わりに器が共通 verify と契約の検証行を撃つ周・設計 pipeline.md §66）が赤を渡して runner を
    /// **起こし直す回数の上限**（回）。値 N = 最大 N 回起こし直す（runner の turn は最大 N+1 回）。0 は門を撃って記録するだけで
    /// 起こし直さない。
    RunnerEndGateRounds,
    /// 変異検査の並列度の**上限**（宣言値）。実効値は受付（設計 gate-cost.md §3.3）が導く。
    GateMutantsJobs,
    /// job 1 つが要る memory の宣言値（MiB）。受付の分母と封じ込めの箱に使う。
    GateJobMemoryMb,
    /// 席と host のために常に残す memory（MiB）。受付はこれを差し引いた空きしか配らない。
    HostReserveMemoryMb,
    /// 受付で枠が空くのを待つ上限（秒）。超えたら並列度 1 で進む（縮退・止めない）。
    GateSlotWaitS,
    /// tmux を立てる歯（e2e の isolated seat）の同時本数（本・設計 gate-cost.md §3.1・`s2-07l.360`）。値の写しは
    /// nextest の test-group `tmux` の `max-threads`（`.config/nextest.toml`）で、`cargo xtask check` が写しの一致と
    /// 配線を manifest と突合する（clippy.toml ↔ R-C4-4.* と同型）。読み手は xtask 側（core は値を消費しない）。
    GateTmuxTestThreads,
    /// 便の scope に付ける CPU の重み（席は既定の重み）。
    GateCpuWeight,
    /// 器の健康の遮断器の**走行可能の core あたりの倍率**（設計 gate-cost.md §32）。閾値 = 値 × 実測の core 数で、
    /// `/proc/loadavg` の 4 番目の欄の分子がこれを超えた周は行を撃つ前に空くまで待つ。
    HostRunnablePerCore,
    /// 器の健康の遮断器の**待ちの core あたりの倍率**（設計 gate-cost.md §32）。閾値 = 値 × 実測の core 数で、
    /// `/proc/stat` の `procs_blocked` がこれを超えた周は行を撃つ前に空くまで待つ。
    HostBlockedPerCore,
    /// land が着地待ちの列で自分の番を待つ上限（秒）。超えたら待たずに進む（縮退・止めない）。
    PipeLandWaitS,
    /// 検出線を起こす間隔の下限（秒・設計 gate-cost.md §50）。land の終端は前に口を起こしてからこの秒が過ぎた周だけ
    /// 着地後の検出の口を起こし、内の周は起こさず deferred を記す（日次の検出）。
    DetectionDailyMinS,
    /// land の終端が CI の判定を待つ上限（秒・設計 contract-source.md §5）。超えた周は **close しない**
    /// （`unmeasurable` で止める・FailClosed）。
    PipeCiWaitS,
    /// land の終端が CI の判定を照合する間隔（秒・設計 contract-source.md §50）。[`Self::PipeCiWaitS`] の上限の内側を
    /// この間隔で撃つ（1 回が forge の API の 1 回）。0 は唯一の待ちの既定の周期に戻る。
    PipeCiPollS,
    /// 席の起草の置き場の写しの中間生成物を器が消す**書きの線**（時間・設計 dispatcher.md §33・ADR-0096）。
    /// 起草の木の名が閉じた列の dir は、自身と下の全 entry の最新の書きがこの時間より前のときだけ消える。0 は線 = 今。
    SeatDraftsStaleH,
    /// 席の指示文の `{ledger}` が台帳（`bd --readonly`）の子 process を待つ上限（秒）。超えたら数えを返さない。
    LedgerTimeoutS,
    /// 役割ごとの権能（設計 seat-roles.md §3・ADR-0022 §2.2）。値は権能の名の列で、名の集合は
    /// [`crate::seat::role::Capability`] が閉じる（列に無い名は読み込みで拒む）。**1 kind で行が 2 つ**
    /// （id は `role.<役割名>`・役割ごとに 1 行）。
    RoleCapabilities,
    /// 契約の `size` = S の 1 file あたりの増分の見積（行）。契約表の上限の余地（設計 contract-source.md §3）が読む。
    PipeSizeSLines,
    /// 契約の `size` = M の 1 file あたりの増分の見積（行）。
    PipeSizeMLines,
    /// 契約の `size` = L の 1 file あたりの増分の見積（行）。
    PipeSizeLLines,
    /// runner が claude に**毎回**渡す model（設計 pipeline.md §6 / §61・`s2-07l.297`）。値は claude CLI の別名
    /// （閉じた表は [`crate::fleet::select::Model`]）。便用の口座選定はこの model のモデル別窓だけを数える。
    RunnerModel,
    /// runner / lens が claude に**毎回**渡す effort（設計 pipeline.md §6・`s2-07l.322`）。値は claude CLI の字面
    /// （閉じた表は [`crate::headless::Effort`]）。省くと口座の設定 dir の `settings.json` の値で決まる。
    RunnerEffort,
    /// `--stage` を持たない lens（契約の審査と gate の審査）が claude に**毎回**渡す model（設計 pipeline.md §61）。
    /// 値は [`Self::RunnerModel`] と同じ語彙。
    LensModel,
    /// `--stage prelens` の lens（事前審査の先撃ち）が claude に**毎回**渡す model（設計 pipeline.md §61）。
    /// [`Self::LensModel`] と同じ model に解ける周だけ、先撃ちの判定を契約の審査に使い回す（dispatcher.md §27 形 ac 1）。
    PipePrecheckLensModel,
    /// 役割ごとの既定の model（設計 seat-roles.md §19・`s2-07l.433`）。値は claude CLI の別名か表示名
    /// （閉じた表は [`crate::fleet::select::Model`]・表に無い字面は読み込みで拒む）。**1 kind で行は役割ごとに
    /// 1 つ**（id は `seat.model.<役割名>`）で、[`Self::RoleEffort`] と対で読む。
    RoleModel,
    /// 役割ごとの既定の effort（設計 seat-roles.md §19・`s2-07l.433`）。値は claude CLI の字面（閉じた表は
    /// [`crate::headless::Effort`]・表に無い字面は読み込みで拒む）。id は `seat.effort.<役割名>`。
    RoleEffort,
    /// 同型の審査 FAIL で run N+1 を止める回数（本・設計 contract-source.md §23・`s2-07l.396`）。受付は同じ bead の
    /// 便を新しい順に読み、同じ理由の型（`FindingKind`）の FAIL が PASS で途切れるまでこの本数続き、契約 file と
    /// 節の本文がともに不変の周を `same-kind-repeated` で断る。
    ReviewSameKindStop,
    /// 着地の列を候補の木 1 つに積む本数の上限（本・先頭を含む・設計 pipeline.md §40・ADR-0039）。値 1 と
    /// 行の不在は先頭だけ（列を積まない＝従来の経路）。
    LandTrainMax,
    /// host で同時に走る便（live な便）の本数の最大値（本・設計 gate-cost.md §24・ADR-0035）。受付は便を作る前に
    /// live な便を数え、この値以上の周を `max-live` で断る（走行中の便には効かない）。
    PipeMaxLive,
    /// 入口の flip check が `.rs` の差の無い便を docs-only と読む **path の面**（設計 pipeline.md §7・`s2-07l.170`）。
    /// 値は path の列（`/` で終わる要素は接頭辞・他は完全一致）で、面の外の file を含む便は `no-test-diff` で落ちる。
    /// 読み手は xtask 側（core は値を消費しない）。
    FlipDocsOnlyFaces,
    /// 1 便が足してよい flip check の札（`retroactive` / `moved`）の本数の上限（本・設計 pipeline.md §7・
    /// `s2-07l.170`）。超えた便は `too-many-marks` で落ちる。読み手は xtask 側（core は値を消費しない）。
    FlipMarksPerPr,
    /// 起票の門が断る台帳 write の形（設計 vessel-hook.md §10・`s2-07l.169`）。値は形の 1 語の閉じた列
    /// （`notes-replace` / `memory-subcommand` / `create-without-parent` / `bd-outside-bdw` / `create-bypass` /
    /// `parent-edge`・後ろの 2 語は設計 ledger-form.md §11）で、判定そのものは
    /// [`crate::hook::ledger_guard`] が持つ。列に載る形だけを断る。
    LedgerDeniedWrites,
    /// 台帳のグラフの直下の open の子の上限（本・設計 ledger-form.md §10 形 4）。doctor の台帳のグラフの行
    /// （[`crate::ledger::graph`]）が id で引いて整数だけを読み、越えた親を over に名指す。0 は over を数えない。
    LedgerOpenChildrenMax,
    /// host の破壊防止の見張りの語列（設計 vessel-hook.md §11 行 b・ADR-0056）。値は [`Self::RunnerDeniedCommands`] と
    /// 同じ形の語列の配列で、**1 kind で行が 3 つ**（id は [`crate::hook::host_guard::WORD_ROWS`]・種類ごとに 1 行）。
    HostGuardDeniedCommands,
    /// host の破壊防止の見張りの rm の守る集合（設計 vessel-hook.md §11 行 b / c）。値は守る集合の記号の列（各語を
    /// [`crate::hook::host_guard::Protected`] で引く）。id は `host_guard.rm` の 1 行。
    HostGuardRmProtected,
    /// 管理 tick の timer の周期（秒・設計 seat-heartbeat.md §2 形 5・ADR-0058 §2）。unit を書く口が読み、tick の判定は
    /// 読むだけで使わない（行が読めない周は `no-rule`）。
    SeatTickIntervalS,
    /// 管理 tick の黙りの閾値（settle の基準）・Busy の古さの 2 役（秒・[`crate::seat::tick`]）。
    SeatTickStaleS,
    /// 合図の梯子の列（秒の文字列の列・非空・狭義に昇順・設計 seat-heartbeat.md §10 形 5・ADR-0068）。段 n の待ちは列の
    /// n 番目で、列を越えた段は送らない（`stopped`）。数でない・昇順でない列は tick の読みが `no-rule` で断る。
    SeatPointerLadderS,
    /// 群の移動の退避の猶予（秒・起点は群の記録の ts・設計 seat-heartbeat.md §13 形 1・ADR-0071）。猶予の内側は退避の合図だけを
    /// 送り `/exit` は越えてから送る。0 は猶予なし。読めない周は tick が `no-rule` で断る。
    SeatMoveGraceS,
    /// 席の起動を包む封じ込めの箱の memory の上限（MiB・設計 account-lifecycle.md §30 形 1・ADR-0072）。0 は包まない・読めない周も
    /// 包まない（起動は止めない＝縮退）。
    SeatMemoryMaxMb,
    /// heartbeat の段の上げの閾値（秒・設計 seat-heartbeat.md §17 形 1）。live 0 本の分数 × 60 がこの値以上の周は黙りの門を
    /// 短くし梯子を段 0 に留める。0 は上げない。任意の行で、読めない周は上げず合図に `alarm=idle-unset` を足す。
    SeatIdleAlarmS,
    /// 事前審査の確定の束の段の上げの閾値（秒・設計 dispatcher.md §27 形 4）。最も古い確定の束の初めて見た時刻からこの値以上
    /// 経った周は `SeatIdleAlarmS` と同じ段の上げを撃つ。0 は上げない。任意の行で、読めない周は上げず `alarm=precheck-unset`。
    SeatPrecheckAlarmS,
    /// **クラスの語列表**（設計 contract-source.md §48 の 2・ADR-0061）。値は要素「クラスの名 + 語列」の列（読み手は
    /// [`crate::pipe::contract::class_element`] の 1 本）で、契約表の検査が verify 各行に禁じる語列と同じ照合で当て、導出が
    /// 行の `classes` に無い行を断る。id は [`crate::pipe::contract::CLASS_ROW`] の 1 行。
    RunnerClassCommands,
    /// host-guard の公開の見張りの行（設計 vessel-hook.md §16 形 6・ADR-0078）。値は札つきの要素 `form <記号>` の
    /// 列（読み手は [`publish::elements`] の 1 本）。頭の語が `exclude` の要素は置けない（除外は host の面の表
    /// `[[publish-exclusion]]`・兄弟 [`exclusion`]・ADR-0093）。
    HostGuardPublish,
    /// 床の検査（vessel 宣言の任意 key `floor-check`）を待つ上限（秒・設計 dispatcher.md §34 約束 4）。越えた周は子を止めて timeout と読む。
    /// 読み手は `pipe::dispatch::floor` の 1 本で、行を読めない周は撃たず unfireable（`row`）。
    FloorTimeoutS,
    /// 行の予約の期限（時間・設計 row-review.md §7）。落ちた契約の終端の段の event の ts からこの時間を過ぎた行は行の予約を持たない。
    /// 値 0 は期限なし。行を読めない周は期限なしで予約を掛け、reserved の値の末尾に `/unset` を足す。読み手は `pipe::dispatch::reserve` の 1 本。
    PipeReserveH,
    /// 席の起草の置き場の build の置き場の量の上限（MiB・state dir ごと・設計 dispatcher.md §39・ADR-0101）。書きの線が残した
    /// 起草の木の dir の合計がこれを越える周に、書きの新しさの古い順に上限まで消す。値 0 は窓の外の候補を全部消す。
    SeatDraftsCapMb,
    /// 量の線が消さない組み立て中の窓（秒・設計 dispatcher.md §39 形 5）。新しさがこの秒数以内の候補は上限を越えても消さない。0 は窓無し。
    SeatDraftsBusyS,
    /// 局面の出力の終わりの局面の閉じた部品を載せる窓（時間・設計 case-lifecycle.md §12 約束 10・ADR-0088）。書き手が窓の秒に直して
    /// 部品の判定へ渡す。id は `lifecycle.closed_window_h` の 1 行。
    LifecycleClosedWindowH,
    /// 手番が seat の局面が滞ったとみなす年齢の閾値（時間・§12 約束 10）。**1 kind で行が 13 本**で、id は `lifecycle.age_h.<語>`
    /// （語は手番が seat の局面の語・`age_word_is_known` が語の外の後ろを断る）。行の無い seat の語は `owned.unset` に数える。
    LifecycleAgeH,
    /// 管理 tick の全部の書き直しの下限（秒・設計 case-lifecycle.md §19 約束 1）。前の全部の書き直しと tick の撃った記録の新しい方からこの秒数以上後の周だけ撃つ。id は `lifecycle.full_min_s`。
    LifecycleFullMinS,
    /// open な memo の notes の byte の上限（設計 ledger-form.md §19 約束 4・FR87）。越えた open な memo を doctor の台帳の行が名指す
    /// （門では止めない）。行を読めない周は `oversized=no-rule`。読み手は `ledger::lint` の 1 本。
    MemoNotesMaxBytes,
    /// 1 本の memo を審査にかける間隔（時間・設計 ledger-form.md §19 約束 5・読み手は dispatcher.md 行 aq）。
    MemoTriageIntervalH,
    /// 1 周に審査にかける memo の本数の上限（設計 ledger-form.md §19 約束 5・読み手は dispatcher.md 行 aq）。
    MemoTriagePerRound,
    /// code の索引の置き場の量の上限（MiB・state dir ごと・設計 reverse-index.md §4 形 7・ADR-0101 と同じ形）。置き場の合計がこれを
    /// 越える周に、撃ち中の鍵と anchor の HEAD の鍵を除いて記録の at の古い順に上限まで消す。読み手は `pipe::dispatch::index_build` の 1 本。
    IndexCapMb,
    /// code の索引の外の道具 1 本を待つ上限（秒・設計 reverse-index.md §4 形 2・形 9）。越えた子は process group ごと止めて failed:timeout と読む。
    /// 撃ち中の持ち主の終わりを待つ上限も同じ値。読み手は `pipe::dispatch::index_build` の 1 本。
    IndexTimeoutS,
}

/// [`RuleKind`] の全 variant。parity test の母集団である。
pub const ALL: &[RuleKind] = &[
    RuleKind::CoreLines,
    RuleKind::ModuleLines,
    RuleKind::TestSrcRatioPct,
    RuleKind::FnLines,
    RuleKind::FnComplexity,
    RuleKind::FnArgs,
    RuleKind::LineWidth,
    RuleKind::BoundaryLines,
    RuleKind::DialogueSurface,
    RuleKind::MaturityCondition,
    RuleKind::AccountSelection,
    RuleKind::MutationSurvivalLine,
    RuleKind::DepBudget,
    RuleKind::DepPerPr,
    RuleKind::CheckDeltaMs,
    RuleKind::CompileShape,
    RuleKind::CompileSeconds,
    RuleKind::GateLensCount,
    RuleKind::PipePrecheckLensPerRound,
    RuleKind::GateTokenCap,
    RuleKind::RunTokenCeiling,
    RuleKind::LensMaxTurns,
    RuleKind::PipePermitRows,
    RuleKind::PipePermitMaxH,
    RuleKind::HookBudgetMs,
    RuleKind::HostGuardPublishDeadlineMs,
    RuleKind::HostGuardPublishReadBytes,
    RuleKind::StopGraceMs,
    RuleKind::LockRetryMs,
    RuleKind::LockStaleMs,
    RuleKind::HookTimeoutS,
    RuleKind::RunnerAllowedCommands,
    RuleKind::RunnerDeniedCommands,
    RuleKind::RepoNonRustExecAllow,
    RuleKind::SeatCycleSettleS,
    RuleKind::SeatCyclePollMs,
    RuleKind::UsageTimeoutS,
    RuleKind::UsageFreshS,
    RuleKind::GroupPressure5hPct,
    RuleKind::GroupPressure7dPct,
    RuleKind::GroupPressureModelPct,
    RuleKind::FollowRetries,
    RuleKind::RunnerEndGateRounds,
    RuleKind::GateMutantsJobs,
    RuleKind::GateJobMemoryMb,
    RuleKind::HostReserveMemoryMb,
    RuleKind::GateSlotWaitS,
    RuleKind::GateTmuxTestThreads,
    RuleKind::GateCpuWeight,
    RuleKind::HostRunnablePerCore,
    RuleKind::HostBlockedPerCore,
    RuleKind::PipeLandWaitS,
    RuleKind::DetectionDailyMinS,
    RuleKind::PipeCiWaitS,
    RuleKind::PipeCiPollS,
    RuleKind::SeatDraftsStaleH,
    RuleKind::LedgerTimeoutS,
    RuleKind::RoleCapabilities,
    RuleKind::PipeSizeSLines,
    RuleKind::PipeSizeMLines,
    RuleKind::PipeSizeLLines,
    RuleKind::RunnerModel,
    RuleKind::RunnerEffort,
    RuleKind::LensModel,
    RuleKind::PipePrecheckLensModel,
    RuleKind::RoleModel,
    RuleKind::RoleEffort,
    RuleKind::ReviewSameKindStop,
    RuleKind::LandTrainMax,
    RuleKind::PipeMaxLive,
    RuleKind::FlipDocsOnlyFaces,
    RuleKind::FlipMarksPerPr,
    RuleKind::LedgerDeniedWrites,
    RuleKind::LedgerOpenChildrenMax,
    RuleKind::HostGuardDeniedCommands,
    RuleKind::HostGuardRmProtected,
    RuleKind::SeatTickIntervalS,
    RuleKind::SeatTickStaleS,
    RuleKind::SeatPointerLadderS,
    RuleKind::SeatMoveGraceS,
    RuleKind::SeatMemoryMaxMb,
    RuleKind::SeatIdleAlarmS,
    RuleKind::SeatPrecheckAlarmS,
    RuleKind::RunnerClassCommands,
    RuleKind::HostGuardPublish,
    RuleKind::FloorTimeoutS,
    RuleKind::PipeReserveH,
    RuleKind::SeatDraftsCapMb,
    RuleKind::SeatDraftsBusyS,
    RuleKind::LifecycleClosedWindowH,
    RuleKind::LifecycleAgeH,
    RuleKind::LifecycleFullMinS,
    RuleKind::MemoNotesMaxBytes,
    RuleKind::MemoTriageIntervalH,
    RuleKind::MemoTriagePerRound,
    RuleKind::IndexCapMb,
    RuleKind::IndexTimeoutS,
];

impl RuleKind {
    /// manifest の `kind` に書く字面（variant 名と一致）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CoreLines => "CoreLines",
            Self::ModuleLines => "ModuleLines",
            Self::TestSrcRatioPct => "TestSrcRatioPct",
            Self::FnLines => "FnLines", Self::FnComplexity => "FnComplexity", Self::FnArgs => "FnArgs",
            Self::LineWidth => "LineWidth", Self::BoundaryLines => "BoundaryLines",
            Self::DialogueSurface => "DialogueSurface",
            Self::MaturityCondition => "MaturityCondition", Self::AccountSelection => "AccountSelection",
            Self::MutationSurvivalLine => "MutationSurvivalLine",
            Self::DepBudget => "DepBudget",
            Self::DepPerPr => "DepPerPr",
            Self::CheckDeltaMs => "CheckDeltaMs",
            Self::CompileShape => "CompileShape",
            Self::CompileSeconds => "CompileSeconds",
            Self::GateLensCount => "GateLensCount", Self::PipePrecheckLensPerRound => "PipePrecheckLensPerRound",
            Self::GateTokenCap => "GateTokenCap", Self::RunTokenCeiling => "RunTokenCeiling", Self::LensMaxTurns => "LensMaxTurns",
            Self::PipePermitRows => "PipePermitRows", Self::PipePermitMaxH => "PipePermitMaxH",
            Self::HookBudgetMs => "HookBudgetMs", Self::HostGuardPublishDeadlineMs => "HostGuardPublishDeadlineMs", Self::HostGuardPublishReadBytes => "HostGuardPublishReadBytes",
            Self::StopGraceMs => "StopGraceMs", Self::LockRetryMs => "LockRetryMs",
            Self::LockStaleMs => "LockStaleMs",
            Self::HookTimeoutS => "HookTimeoutS",
            Self::RunnerAllowedCommands => "RunnerAllowedCommands",
            Self::RunnerDeniedCommands => "RunnerDeniedCommands", Self::RunnerClassCommands => "RunnerClassCommands",
            Self::RepoNonRustExecAllow => "RepoNonRustExecAllow",
            Self::SeatCycleSettleS => "SeatCycleSettleS",
            Self::SeatCyclePollMs => "SeatCyclePollMs",
            Self::UsageTimeoutS => "UsageTimeoutS",
            Self::UsageFreshS => "UsageFreshS",
            // 群の逼迫の 3 行は 2 行に、契約の size の 3 行は 2 行に、host の見張りの 2 kind と token の上限の 2 kind は 1 行に畳む（関数 1 本の行数の
            // 上限 R-C4-4.fn-lines・閉じた列の網羅は不変）。
            Self::GroupPressure5hPct => "GroupPressure5hPct", Self::GroupPressure7dPct => "GroupPressure7dPct",
            Self::GroupPressureModelPct => "GroupPressureModelPct",
            Self::FollowRetries => "FollowRetries", Self::RunnerEndGateRounds => "RunnerEndGateRounds",
            Self::GateMutantsJobs => "GateMutantsJobs",
            Self::GateJobMemoryMb => "GateJobMemoryMb",
            Self::HostReserveMemoryMb => "HostReserveMemoryMb",
            Self::GateSlotWaitS => "GateSlotWaitS",
            Self::GateTmuxTestThreads => "GateTmuxTestThreads",
            Self::GateCpuWeight => "GateCpuWeight",
            Self::HostRunnablePerCore => "HostRunnablePerCore", Self::HostBlockedPerCore => "HostBlockedPerCore",
            Self::PipeLandWaitS => "PipeLandWaitS", Self::DetectionDailyMinS => "DetectionDailyMinS",
            Self::PipeCiWaitS => "PipeCiWaitS", Self::PipeCiPollS => "PipeCiPollS",
            Self::SeatDraftsStaleH => "SeatDraftsStaleH", Self::SeatDraftsCapMb => "SeatDraftsCapMb", Self::SeatDraftsBusyS => "SeatDraftsBusyS",
            Self::LedgerTimeoutS => "LedgerTimeoutS",
            Self::RoleCapabilities => "RoleCapabilities",
            Self::PipeSizeSLines => "PipeSizeSLines", Self::PipeSizeMLines => "PipeSizeMLines",
            Self::PipeSizeLLines => "PipeSizeLLines",
            Self::RunnerModel => "RunnerModel", Self::RunnerEffort => "RunnerEffort", Self::LensModel => "LensModel",
            Self::PipePrecheckLensModel => "PipePrecheckLensModel",
            Self::RoleModel => "RoleModel", Self::RoleEffort => "RoleEffort",
            Self::ReviewSameKindStop => "ReviewSameKindStop",
            Self::LandTrainMax => "LandTrainMax",
            Self::PipeMaxLive => "PipeMaxLive",
            Self::FlipDocsOnlyFaces => "FlipDocsOnlyFaces", Self::FlipMarksPerPr => "FlipMarksPerPr",
            Self::LedgerDeniedWrites => "LedgerDeniedWrites", Self::LedgerOpenChildrenMax => "LedgerOpenChildrenMax",
            Self::HostGuardDeniedCommands => "HostGuardDeniedCommands", Self::HostGuardRmProtected => "HostGuardRmProtected", Self::HostGuardPublish => "HostGuardPublish",
            // 管理 tick の 3 kind と席の箱も 2 行に畳み、対で読む model と effort の 2 組も 1 行ずつに畳む（同じ上限）。
            Self::SeatTickIntervalS => "SeatTickIntervalS", Self::SeatTickStaleS => "SeatTickStaleS", Self::SeatMemoryMaxMb => "SeatMemoryMaxMb",
            Self::SeatPrecheckAlarmS => "SeatPrecheckAlarmS", Self::FloorTimeoutS => "FloorTimeoutS", Self::PipeReserveH => "PipeReserveH",
            Self::LifecycleClosedWindowH => "LifecycleClosedWindowH", Self::LifecycleAgeH => "LifecycleAgeH", Self::LifecycleFullMinS => "LifecycleFullMinS", Self::MemoNotesMaxBytes => "MemoNotesMaxBytes",
            Self::MemoTriageIntervalH => "MemoTriageIntervalH", Self::MemoTriagePerRound => "MemoTriagePerRound", Self::IndexCapMb => "IndexCapMb", Self::IndexTimeoutS => "IndexTimeoutS", Self::SeatPointerLadderS => "SeatPointerLadderS",
            Self::SeatMoveGraceS => "SeatMoveGraceS", Self::SeatIdleAlarmS => "SeatIdleAlarmS",
        }
    }

    /// 種類が要求する値の形。**対応はこの `match` ただ 1 箇所**が持つ。
    pub fn shape(self) -> ValueShape {
        match self {
            Self::CoreLines
            | Self::ModuleLines
            | Self::TestSrcRatioPct
            | Self::FnLines
            | Self::FnComplexity
            | Self::FnArgs
            | Self::LineWidth | Self::BoundaryLines
            | Self::DepBudget
            | Self::DepPerPr
            | Self::CheckDeltaMs
            | Self::GateLensCount | Self::PipePrecheckLensPerRound
            | Self::GateTokenCap | Self::RunTokenCeiling | Self::LensMaxTurns | Self::PipePermitMaxH
            | Self::HookBudgetMs | Self::HostGuardPublishDeadlineMs | Self::HostGuardPublishReadBytes
            | Self::StopGraceMs | Self::LockRetryMs
            | Self::LockStaleMs
            | Self::HookTimeoutS
            | Self::SeatCycleSettleS
            | Self::SeatCyclePollMs
            | Self::UsageTimeoutS
            | Self::UsageFreshS
            | Self::GroupPressure5hPct | Self::GroupPressure7dPct | Self::GroupPressureModelPct
            | Self::FollowRetries | Self::RunnerEndGateRounds
            | Self::GateMutantsJobs
            | Self::GateJobMemoryMb
            | Self::HostReserveMemoryMb
            | Self::GateSlotWaitS
            | Self::GateTmuxTestThreads
            | Self::GateCpuWeight
            | Self::HostRunnablePerCore
            | Self::HostBlockedPerCore
            | Self::PipeLandWaitS | Self::DetectionDailyMinS
            | Self::PipeCiWaitS | Self::PipeCiPollS | Self::SeatDraftsStaleH | Self::SeatDraftsCapMb | Self::SeatDraftsBusyS
            | Self::LedgerTimeoutS
            | Self::PipeSizeSLines
            | Self::PipeSizeMLines
            | Self::PipeSizeLLines
            | Self::ReviewSameKindStop
            | Self::LandTrainMax
            | Self::PipeMaxLive
            | Self::FlipMarksPerPr | Self::LedgerOpenChildrenMax
            | Self::SeatTickIntervalS | Self::SeatTickStaleS | Self::SeatMoveGraceS | Self::SeatMemoryMaxMb | Self::SeatIdleAlarmS
            | Self::SeatPrecheckAlarmS | Self::AccountSelection | Self::FloorTimeoutS | Self::PipeReserveH
            | Self::LifecycleClosedWindowH | Self::LifecycleAgeH | Self::LifecycleFullMinS | Self::MemoNotesMaxBytes | Self::MemoTriageIntervalH | Self::MemoTriagePerRound | Self::IndexCapMb | Self::IndexTimeoutS => ValueShape::Int,
            Self::DialogueSurface
            | Self::RunnerModel
            | Self::RunnerEffort | Self::LensModel | Self::PipePrecheckLensModel
            | Self::RoleModel
            | Self::RoleEffort => ValueShape::Str,
            Self::MaturityCondition
            | Self::MutationSurvivalLine
            | Self::CompileShape
            | Self::CompileSeconds => ValueShape::Policy,
            Self::RunnerAllowedCommands
            | Self::RunnerDeniedCommands | Self::RunnerClassCommands
            | Self::RepoNonRustExecAllow
            | Self::RoleCapabilities
            | Self::FlipDocsOnlyFaces | Self::HostGuardPublish
            | Self::LedgerDeniedWrites | Self::HostGuardDeniedCommands | Self::HostGuardRmProtected | Self::SeatPointerLadderS | Self::PipePermitRows => ValueShape::List,
        }
    }

    /// 上限の許可の読み手を持つ kind か（設計 limit-permit.md §18 約束 2）。**分けはこの `match` ただ 1 箇所**が持ち、wildcard `_` を
    /// 書かない（kind を足した周にどの分けかを決めないと compile が落ちる）。true は `GateTokenCap` だけで、ほかの作業ごとの消費の行は
    /// 読み手を足す後継の決定の後に true へ移す。
    pub fn has_permit_reader(self) -> bool {
        match self {
            Self::GateTokenCap => true,
            Self::CoreLines | Self::ModuleLines | Self::TestSrcRatioPct | Self::FnLines | Self::FnComplexity | Self::FnArgs
            | Self::LineWidth | Self::BoundaryLines | Self::DialogueSurface | Self::MaturityCondition | Self::AccountSelection
            | Self::MutationSurvivalLine | Self::DepBudget | Self::DepPerPr | Self::CheckDeltaMs | Self::CompileShape
            | Self::CompileSeconds | Self::GateLensCount | Self::PipePrecheckLensPerRound | Self::RunTokenCeiling
            | Self::LensMaxTurns | Self::PipePermitRows | Self::PipePermitMaxH | Self::HookBudgetMs
            | Self::HostGuardPublishDeadlineMs | Self::HostGuardPublishReadBytes | Self::StopGraceMs | Self::LockRetryMs
            | Self::LockStaleMs | Self::HookTimeoutS | Self::RunnerAllowedCommands | Self::RunnerDeniedCommands
            | Self::RepoNonRustExecAllow | Self::SeatCycleSettleS | Self::SeatCyclePollMs | Self::UsageTimeoutS
            | Self::UsageFreshS | Self::GroupPressure5hPct | Self::GroupPressure7dPct | Self::GroupPressureModelPct
            | Self::FollowRetries | Self::RunnerEndGateRounds | Self::GateMutantsJobs | Self::GateJobMemoryMb
            | Self::HostReserveMemoryMb | Self::GateSlotWaitS | Self::GateTmuxTestThreads | Self::GateCpuWeight
            | Self::HostRunnablePerCore | Self::HostBlockedPerCore | Self::PipeLandWaitS | Self::DetectionDailyMinS
            | Self::PipeCiWaitS | Self::PipeCiPollS | Self::SeatDraftsStaleH | Self::LedgerTimeoutS | Self::RoleCapabilities
            | Self::PipeSizeSLines | Self::PipeSizeMLines | Self::PipeSizeLLines | Self::RunnerModel | Self::RunnerEffort
            | Self::LensModel | Self::PipePrecheckLensModel | Self::RoleModel | Self::RoleEffort | Self::ReviewSameKindStop
            | Self::LandTrainMax | Self::PipeMaxLive | Self::FlipDocsOnlyFaces | Self::FlipMarksPerPr
            | Self::LedgerDeniedWrites | Self::LedgerOpenChildrenMax | Self::HostGuardDeniedCommands
            | Self::HostGuardRmProtected | Self::SeatTickIntervalS | Self::SeatTickStaleS | Self::SeatPointerLadderS
            | Self::SeatMoveGraceS | Self::SeatMemoryMaxMb | Self::SeatIdleAlarmS | Self::SeatPrecheckAlarmS
            | Self::RunnerClassCommands | Self::HostGuardPublish | Self::FloorTimeoutS | Self::PipeReserveH
            | Self::SeatDraftsCapMb | Self::SeatDraftsBusyS | Self::LifecycleClosedWindowH | Self::LifecycleAgeH
            | Self::LifecycleFullMinS | Self::MemoNotesMaxBytes | Self::MemoTriageIntervalH | Self::MemoTriagePerRound
            | Self::IndexCapMb | Self::IndexTimeoutS => false,
        }
    }

    /// manifest の `kind` の字面から種類を引く。未知なら `None`。
    pub fn parse(text: &str) -> Option<Self> {
        ALL.iter().copied().find(|kind| kind.as_str() == text)
    }
}

/// 規則の値。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleValue {
    /// 閾値。
    Int(u64),
    /// 識別子。
    Str(String),
    /// 散文の規則本文。
    Policy(String),
    /// 文字列の列（順序は manifest の並びのまま＝機械が読む順序である）。
    List(Vec<String>),
}

impl RuleValue {
    /// この値の形。
    pub fn shape(&self) -> ValueShape {
        match *self {
            Self::Int(_) => ValueShape::Int,
            Self::Str(_) => ValueShape::Str,
            Self::Policy(_) => ValueShape::Policy,
            Self::List(_) => ValueShape::List,
        }
    }

    /// 1 行で表示する形（CLI の `rules get` が使う）。
    pub fn render(&self) -> String {
        match self {
            Self::Int(value) => value.to_string(),
            Self::Str(text) | Self::Policy(text) => text.clone(),
            // **1 行で区切りが読める形**にする（要素を空白で継ぐと、空白を含む
            // 要素〔共通 verify の 1 行〕が何本あるのか読めなくなる）。
            Self::List(items) => {
                let quoted: Vec<String> = items.iter().map(|item| format!("\"{item}\"")).collect();
                format!("[{}]", quoted.join(", "))
            }
        }
    }
}

/// manifest の 1 行。`ruling` / `ruled_at` は全行必須である（憲法 C5）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleRow {
    /// 行 id。憲法 §3 の行 id が接頭辞として一致する。
    pub id: String,
    /// 規則の種類。
    pub kind: RuleKind,
    /// 規則の値。
    pub value: RuleValue,
    /// false = 値は写すが機械は効かせない。
    pub enabled: bool,
    /// 裁定 id。
    pub ruling: String,
    /// 裁定の日付。
    pub ruled_at: String,
    /// manifest の中でこの行が始まる物理行番号。
    pub line: u64,
}

/// 規則 1 行が満たすべき性質。
pub trait Rule {
    /// この行の種類。
    fn kind(&self) -> RuleKind;
    /// 値の形と裁定の記入を検査する。
    fn validate(&self) -> Result<(), RuleError>;
}

impl Rule for RuleRow {
    fn kind(&self) -> RuleKind {
        self.kind
    }

    fn validate(&self) -> Result<(), RuleError> {
        let want = self.kind.shape();
        let got = self.value.shape();
        if want != got {
            return Err(RuleError::new(
                self.line,
                format!(
                    "{} の value が kind {} の形と合わない（要 {want:?}・実 {got:?}）",
                    self.id,
                    self.kind.as_str()
                ),
            ));
        }
        if self.ruling.is_empty() || self.ruled_at.is_empty() {
            return Err(RuleError::new(
                self.line,
                format!("{} に ruling / ruled_at が無い", self.id),
            ));
        }
        self.names_are_known()
    }
}

impl RuleRow {
    /// 値が**閉じた名の集合**を指す kind は、名を core の enum で引けることまで検査する（設計
    /// seat-roles.md §3 / §19・ADR-0022 §2.2）: `RoleCapabilities` の列は [`Capability`] の名、`DialogueSurface`
    /// の値は [`Role`] の名、`RoleModel` の値は [`Model`] の字面、`RoleEffort` の値は [`Effort`] の字面。
    /// 綴り違いを黙って「権能なし」「対話面なし」「既定なし」に倒さない（NFR4）。
    /// `RunnerDeniedCommands` と `HostGuardDeniedCommands` の各要素は語を 1 つ以上持つ（空白だけの語列は何にも当たらず黙って
    /// 効かない・ADR-0025 §2.1）。`HostGuardRmProtected` の列は [`Protected`] の記号（綴り違いを「守らない」に倒さない）。
    /// `RunnerClassCommands` の要素は「クラスの名 + 語列」（[`Self::class_elements_are_read`]）。
    fn names_are_known(&self) -> Result<(), RuleError> {
        let unknown = |what: &str, name: &str, taken: &[&str]| {
            RuleError::new(
                self.line,
                format!("{} の value に未知の{what} {name}（取るのは {}）", self.id, taken.join(" / ")),
            )
        };
        match (self.kind, &self.value) {
            (RuleKind::RoleCapabilities, RuleValue::List(names)) => {
                let taken: Vec<&str> = CAPABILITIES.iter().map(|found| found.as_str()).collect();
                match names.iter().find(|name| Capability::parse(name).is_none()) {
                    Some(name) => Err(unknown("権能", name, &taken)),
                    None => Ok(()),
                }
            }
            (RuleKind::HostGuardRmProtected, RuleValue::List(names)) => {
                let taken: Vec<&str> = PROTECTED.iter().map(|found| found.as_str()).collect();
                match names.iter().find(|name| Protected::parse(name).is_none()) {
                    Some(name) => Err(unknown("守る集合の記号", name, &taken)),
                    None => Ok(()),
                }
            }
            (RuleKind::DialogueSurface, RuleValue::Str(name)) if Role::parse(name).is_none() => {
                let taken: Vec<&str> = ROLES.iter().map(|found| found.as_str()).collect();
                Err(unknown("役割", name, &taken))
            }
            (RuleKind::RoleModel, RuleValue::Str(name)) if Model::parse(name).is_none() => {
                let taken: Vec<&str> = MODELS.iter().map(|found| found.alias()).collect();
                Err(unknown("model", name, &taken))
            }
            (RuleKind::RoleEffort, RuleValue::Str(name)) if Effort::parse(name).is_none() => {
                let taken: Vec<&str> = EFFORTS.iter().map(|found| found.alias()).collect();
                Err(unknown("effort", name, &taken))
            }
            (RuleKind::RunnerDeniedCommands | RuleKind::HostGuardDeniedCommands, RuleValue::List(sequences)) => {
                match sequences.iter().find(|sequence| sequence.split_whitespace().next().is_none()) {
                    Some(blank) => Err(RuleError::new(
                        self.line,
                        format!("{} の value に語を持たない語列 {blank:?}（各要素は空白区切りの語 1 つ以上）", self.id),
                    )),
                    None => Ok(()),
                }
            }
            (RuleKind::RunnerClassCommands, RuleValue::List(elements)) => self.class_elements_are_read(elements),
            (RuleKind::LifecycleAgeH, _) => self.age_word_is_known(),
            (RuleKind::HostGuardPublish, RuleValue::List(found)) => publish::elements(found).map(drop).map_err(|why| RuleError::new(self.line, format!("{} の value の{why}", self.id))),
            _ => Ok(()),
        }
    }

    /// `lifecycle.age_h.<語>` の語の検査（設計 case-lifecycle.md §12 約束 10）: id が `lifecycle.age_h.` で始まり、後ろが手番が seat の局面の語
    /// （`contract-queued` は理由で手番が決まるので語として取る）であること。語の外の後ろは行番号つきで断る。
    fn age_word_is_known(&self) -> Result<(), RuleError> {
        let word = self.id.strip_prefix(AGE_ID_PREFIX).unwrap_or("");
        let seat = |word: &str| word == Phase::ContractQueued.as_str() || turn_of(word, None) == Some(Turn::Seat);
        if seat(word) {
            return Ok(());
        }
        let taken: Vec<&str> = PHASES.iter().copied().filter(|found| seat(found)).collect();
        Err(RuleError::new(
            self.line,
            format!("{} の id の後ろが手番 seat の局面の語でない（{AGE_ID_PREFIX}<語>・取るのは {}）", self.id, taken.join(" / ")),
        ))
    }

    /// クラスの語列表の要素の形（設計 contract-source.md §48 の 3 の (a)(b)・読み手は [`class_element`] の 1 本）: 先頭語が
    /// クラスの名でない要素と名だけの要素（語列が空）を行番号つきで断る（行 1 つで決まる崩れ・最初の 1 件）。行を跨ぐ
    /// (c)(d)（allowlist と禁じる語列）は manifest の読みの段が撃つ。
    fn class_elements_are_read(&self, elements: &[String]) -> Result<(), RuleError> {
        for element in elements {
            let reason = match class_element(element) {
                ClassElement::Pair(..) => continue,
                ClassElement::Unknown(head) => format!("先頭語 {head:?} がクラスの名でない（取るのは {}）", CLASSES.join(" / ")),
                ClassElement::Bare(class) => format!("クラス {} の語列が空", class.as_str()),
            };
            return Err(RuleError::new(self.line, format!("{} の value の要素 {element:?} の{reason}", self.id)));
        }
        Ok(())
    }
}

/// 1 件 1 行で表示する読み取り error。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleError {
    /// 違反が始まる物理行番号。
    pub line: u64,
    /// 違反の説明。
    pub message: String,
}

impl RuleError {
    /// 行番号と説明から作る。
    pub fn new(line: u64, message: String) -> Self {
        Self { line, message }
    }
}

impl std::fmt::Display for RuleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "rules: {} line={}", self.message, self.line)
    }
}

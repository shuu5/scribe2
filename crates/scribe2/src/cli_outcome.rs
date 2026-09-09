//! subcommand 1 回の結果を表す**唯一の型**（憲法 C2「経路は 1 本・語彙は 1 つ」）。
//!
//! rc を `bool` でなく `u8` で持つのは、`fleet` が 0 / 1 / 2 の 3 値を要るためである
//! （設計 fleet-event-log.md §5）。2 値で足りる subcommand も同じ型を返し、bin 側の
//! dispatch はこの 1 型だけを知る。

/// 成功の rc。
pub const RC_OK: u8 = 0;
/// 断りの rc（使い方の誤り・対象が無い・不発効）。
pub const RC_REFUSED: u8 = 1;
/// 読めない store など、対象そのものが壊れているときの rc。
pub const RC_BROKEN: u8 = 2;

/// subcommand 1 回の結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// stdout へ書く行。
    pub out: Vec<String>,
    /// stderr へ書く行。
    pub err: Vec<String>,
    /// 終了コード。
    pub rc: u8,
}

impl Outcome {
    /// stdout へ行を出し rc 0。
    pub fn ok(out: Vec<String>) -> Self {
        Self {
            out,
            err: Vec::new(),
            rc: RC_OK,
        }
    }

    /// stdout へ 1 行だけ出し rc 0。
    pub fn ok_line(line: String) -> Self {
        Self::ok(vec![line])
    }

    /// stderr へ行を出し rc を立てる。**stdout へは 1 byte も書かない**。
    pub fn failed(rc: u8, err: Vec<String>) -> Self {
        Self {
            out: Vec::new(),
            err,
            rc,
        }
    }

    /// stderr へ 1 行だけ出し rc を立てる。
    pub fn failed_line(rc: u8, line: String) -> Self {
        Self::failed(rc, vec![line])
    }
}

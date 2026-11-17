//! 页面：列什么、怎么排、哪些按键。

use items_contracts::{Item, Kind, Status};
use tui_contract::{ActionSpec, PageSpec};

/// 五个页面。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Inbox,
    Todo,
    Idea,
    ReadLater,
    Done,
}

/// 按键对应的操作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    MoveTo(Kind),
    SetStatus(Status),
    Delete,
    EditText,
    SetDue,
    /// `true` 往上。
    Reorder(bool),
    OpenLink,
    OpenImage,
}

impl Page {
    pub const ALL: [Self; 5] = [
        Self::Inbox,
        Self::Todo,
        Self::Idea,
        Self::ReadLater,
        Self::Done,
    ];

    pub const fn id(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Todo => "todo",
            Self::Idea => "idea",
            Self::ReadLater => "read",
            Self::Done => "done",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.id() == id)
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::Inbox => "收件箱",
            Self::Todo => "待办",
            Self::Idea => "想法",
            Self::ReadLater => "稍后读",
            Self::Done => "已完成",
        }
    }

    const fn key(self) -> char {
        match self {
            Self::Inbox => '1',
            Self::Todo => '2',
            Self::Idea => '3',
            Self::ReadLater => '4',
            Self::Done => '5',
        }
    }

    /// 列的是哪个分类（已完成页不限分类）。
    pub const fn kind(self) -> Option<Kind> {
        match self {
            Self::Inbox => Some(Kind::Inbox),
            Self::Todo => Some(Kind::Todo),
            Self::Idea => Some(Kind::Idea),
            Self::ReadLater => Some(Kind::ReadLater),
            Self::Done => None,
        }
    }

    pub const fn status(self) -> Status {
        match self {
            Self::Done => Status::Done,
            _ => Status::Open,
        }
    }

    /// 收件箱、已完成按时间排（最新在上），不能手动挪；其余按手动排序。
    pub const fn reorderable(self) -> bool {
        matches!(self, Self::Todo | Self::Idea | Self::ReadLater)
    }

    /// 这一页的按键和对应的操作。
    pub fn ops(self) -> Vec<(char, &'static str, bool, Op)> {
        let mut ops: Vec<(char, &'static str, bool, Op)> = Vec::new();
        if self == Self::Done {
            ops.push(('u', "恢复", false, Op::SetStatus(Status::Open)));
        } else {
            for (key, label, kind) in [
                ('t', "待办", Kind::Todo),
                ('i', "想法", Kind::Idea),
                ('r', "稍后读", Kind::ReadLater),
                ('b', "收件箱", Kind::Inbox),
            ] {
                if self.kind() != Some(kind) {
                    ops.push((key, label, false, Op::MoveTo(kind)));
                }
            }
            ops.push(('x', "完成", false, Op::SetStatus(Status::Done)));
            ops.push(('a', "归档", false, Op::SetStatus(Status::Archived)));
            ops.push(('e', "改", false, Op::EditText));
            ops.push(('D', "截止", false, Op::SetDue));
        }
        if self.reorderable() {
            ops.push(('K', "上移", false, Op::Reorder(true)));
            ops.push(('J', "下移", false, Op::Reorder(false)));
        }
        ops.push(('o', "打开链接", false, Op::OpenLink));
        ops.push(('O', "看截图", false, Op::OpenImage));
        ops.push(('d', "删除", true, Op::Delete));
        ops
    }

    pub fn op(self, key: char) -> Option<Op> {
        self.ops().into_iter().find(|o| o.0 == key).map(|o| o.3)
    }

    pub fn spec(self) -> PageSpec {
        PageSpec {
            id: self.id().to_owned(),
            title: self.title().to_owned(),
            key: self.key(),
            order: i32::from(self.key() as u8),
            input: None,
            actions: self
                .ops()
                .into_iter()
                .map(|(key, label, confirm, _)| ActionSpec {
                    key,
                    label: label.to_owned(),
                    confirm,
                })
                .collect(),
        }
    }

    /// 这一页的顺序。`items` 是存储按分类和状态列出来的。
    pub fn sort(self, items: &mut [Item]) {
        match self {
            Self::Inbox => items.sort_by_key(|i| std::cmp::Reverse(i.created_at)),
            Self::Done => items.sort_by_key(|i| std::cmp::Reverse(i.updated_at)),
            Self::Todo | Self::Idea | Self::ReadLater => items.sort_by_key(|i| i.rank),
        }
    }
}

/// `J` / `K`：`id` 在 `ids`（这一页的顺序）里往上或往下挪一格，要排到谁前面。
/// 外层 `None` 表示已经到头、不用挪；内层 `None` 表示排到最后。
pub fn neighbour(ids: &[String], id: &str, up: bool) -> Option<Option<String>> {
    let at = ids.iter().position(|i| i == id)?;
    if up {
        let prev = at.checked_sub(1)?;
        Some(Some(ids[prev].clone()))
    } else {
        if at + 1 >= ids.len() {
            return None;
        }
        Some(ids.get(at + 2).cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_per_page() {
        let keys = |p: Page| p.ops().iter().map(|o| o.0).collect::<String>();
        assert_eq!(keys(Page::Inbox), "tirxaeDoOd");
        assert_eq!(keys(Page::Todo), "irbxaeDKJoOd");
        assert_eq!(keys(Page::Done), "uoOd");
        assert_eq!(Page::Todo.op('t'), None, "待办页没有「移到待办」");
        assert_eq!(Page::Todo.op('K'), Some(Op::Reorder(true)));
        assert_eq!(Page::Inbox.op('K'), None, "收件箱按时间排，不能挪");
        let spec = Page::Inbox.spec();
        assert_eq!((spec.key, spec.title.as_str()), ('1', "收件箱"));
        assert!(spec.actions.iter().find(|a| a.key == 'd').unwrap().confirm);
        // 每页的按键不重复
        for p in Page::ALL {
            let mut k: Vec<char> = p.ops().iter().map(|o| o.0).collect();
            k.sort_unstable();
            k.dedup();
            assert_eq!(k.len(), p.ops().len(), "{p:?}");
        }
    }

    #[test]
    fn ids_round_trip() {
        for p in Page::ALL {
            assert_eq!(Page::from_id(p.id()), Some(p));
        }
        assert_eq!(Page::from_id("x"), None);
    }

    #[test]
    fn neighbours_for_moving() {
        let ids: Vec<String> = ["a", "b", "c", "d"].map(str::to_owned).to_vec();
        assert_eq!(neighbour(&ids, "c", true), Some(Some("b".to_owned())));
        assert_eq!(neighbour(&ids, "a", true), None, "已经在最上面");
        assert_eq!(neighbour(&ids, "b", false), Some(Some("d".to_owned())));
        assert_eq!(neighbour(&ids, "c", false), Some(None), "挪到最后");
        assert_eq!(neighbour(&ids, "d", false), None, "已经在最下面");
        assert_eq!(neighbour(&ids, "z", true), None);
    }
}

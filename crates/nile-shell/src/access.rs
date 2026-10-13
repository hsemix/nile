//! Accessibility for the windowed shell: the engine's `A11yTree` as an AccessKit tree, and
//! AccessKit's requests as `A11yAction`s. AccessKit talks to the platform (UI Automation on
//! Windows, NSAccessibility on macOS, AT-SPI on Linux); nothing here runs unless assistive
//! technology is on.

use accesskit::{Action, ActionData, Node, NodeId, Rect, Role, Toggled, TreeInfo, TreeUpdate};
use nile_engine::a11y::{A11yAction, A11yNode, A11yRole, A11yTree};

fn role(r: A11yRole) -> Role {
    use A11yRole as R;
    match r {
        R::Window => Role::Window,
        R::Document => Role::Document,
        R::GenericContainer => Role::GenericContainer,
        R::Paragraph => Role::Paragraph,
        R::Heading => Role::Heading,
        R::Link => Role::Link,
        R::Button => Role::Button,
        R::CheckBox => Role::CheckBox,
        R::RadioButton => Role::RadioButton,
        R::Switch => Role::Switch,
        R::TextInput => Role::TextInput,
        R::MultilineInput => Role::MultilineTextInput,
        R::NumberInput => Role::SpinButton,
        R::SearchInput => Role::SearchInput,
        R::PasswordInput => Role::PasswordInput,
        R::DateInput => Role::DateInput,
        R::ColorWell => Role::ColorWell,
        R::Slider => Role::Slider,
        R::ComboBox => Role::ComboBox,
        R::ListBox => Role::ListBox,
        R::ListBoxOption => Role::ListBoxOption,
        R::List => Role::List,
        R::ListItem => Role::ListItem,
        R::Image => Role::Image,
        R::Table => Role::Table,
        R::RowGroup => Role::RowGroup,
        R::Row => Role::Row,
        R::Cell => Role::Cell,
        R::ColumnHeader => Role::ColumnHeader,
        R::RowHeader => Role::RowHeader,
        R::ProgressBar => Role::ProgressIndicator,
        R::Meter => Role::Meter,
        R::Group => Role::Group,
        R::Dialog => Role::Dialog,
        R::AlertDialog => Role::AlertDialog,
        R::Alert => Role::Alert,
        R::Status => Role::Status,
        R::Menu => Role::Menu,
        R::MenuBar => Role::MenuBar,
        R::MenuItem => Role::MenuItem,
        R::Tab => Role::Tab,
        R::TabList => Role::TabList,
        R::TabPanel => Role::TabPanel,
        R::Toolbar => Role::Toolbar,
        R::Tooltip => Role::Tooltip,
        R::Navigation => Role::Navigation,
        R::Main => Role::Main,
        R::Banner => Role::Banner,
        R::Footer => Role::Footer,
        R::Complementary => Role::Complementary,
        R::Region => Role::Region,
        R::Article => Role::Article,
        R::Form => Role::Form,
        R::Search => Role::Search,
        R::Details => Role::Details,
        R::Disclosure => Role::DisclosureTriangle,
        R::Separator => Role::Splitter,
        R::Blockquote => Role::Blockquote,
        R::Code => Role::Code,
        R::Figure => Role::Figure,
        R::FigureCaption => Role::FigureCaption,
        R::Label => Role::Label,
        R::StaticText => Role::Label,
        R::Iframe => Role::Iframe,
        R::Splitter => Role::Splitter,
    }
}

fn node(n: &A11yNode, scale: f64) -> Node {
    let mut a = Node::new(role(n.role));
    if !n.name.is_empty() {
        a.set_label(n.name.clone());
    }
    if !n.description.is_empty() {
        a.set_description(n.description.clone());
    }
    if let Some(v) = &n.value {
        a.set_value(v.clone());
    }
    if !n.placeholder.is_empty() {
        a.set_placeholder(n.placeholder.clone());
    }
    if let Some((value, min, max, step)) = n.numeric {
        a.set_numeric_value(value);
        a.set_min_numeric_value(min);
        a.set_max_numeric_value(max);
        a.set_numeric_value_step(step);
    }
    let (x, y, w, h) = n.bounds;
    a.set_bounds(Rect { x0: f64::from(x) * scale, y0: f64::from(y) * scale, x1: f64::from(x + w) * scale, y1: f64::from(y + h) * scale });
    a.set_children(n.children.iter().map(|c| NodeId(*c)).collect::<Vec<_>>());
    if n.level > 0 {
        a.set_level(n.level as usize);
    }
    if n.disabled {
        a.set_disabled();
    }
    if n.required {
        a.set_required();
    }
    if n.read_only {
        a.set_read_only();
    }
    if n.selected {
        a.set_selected(true);
    }
    if let Some(e) = n.expanded {
        a.set_expanded(e);
    }
    if let Some(c) = n.checked {
        a.set_toggled(match c {
            Some(true) => Toggled::True,
            Some(false) => Toggled::False,
            None => Toggled::Mixed,
        });
    }
    if !n.link_target.is_empty() {
        a.set_url(n.link_target.clone());
    }
    if n.focusable {
        a.add_action(Action::Focus);
    }
    if n.clickable {
        a.add_action(Action::Click);
    }
    if matches!(n.role, A11yRole::Slider | A11yRole::NumberInput) && !n.disabled {
        a.add_action(Action::Increment);
        a.add_action(Action::Decrement);
    }
    if matches!(n.role, A11yRole::TextInput | A11yRole::SearchInput | A11yRole::MultilineInput | A11yRole::NumberInput) && !n.disabled && !n.read_only {
        a.add_action(Action::SetValue);
    }
    a.add_action(Action::ScrollIntoView);
    a
}

/// The whole tree as one update (AccessKit diffs it against what it already has).
pub fn update(tree: &A11yTree, scale: f64) -> TreeUpdate {
    TreeUpdate {
        nodes: tree.nodes.iter().map(|n| (NodeId(n.id), node(n, scale))).collect(),
        tree: Some(TreeInfo { toolkit_name: Some("Nile".into()), ..TreeInfo::new(NodeId(tree.root)) }),
        tree_id: accesskit::TreeId::ROOT,
        focus: NodeId(tree.focus),
    }
}

/// An AccessKit request as an engine action, with the node it is for.
pub fn action(request: &accesskit::ActionRequest) -> Option<(u64, A11yAction)> {
    let a = match request.action {
        Action::Click => A11yAction::Click,
        Action::Focus => A11yAction::Focus,
        Action::Increment => A11yAction::Increment,
        Action::Decrement => A11yAction::Decrement,
        Action::ScrollIntoView => A11yAction::ScrollIntoView,
        Action::SetValue | Action::ReplaceSelectedText => match &request.data {
            Some(ActionData::Value(v)) => A11yAction::SetValue(v.to_string()),
            _ => return None,
        },
        _ => return None,
    };
    Some((request.target_node.0, a))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nile_engine::Page;

    /// The engine's tree becomes an AccessKit tree that AccessKit accepts: every node reachable,
    /// children present, focus valid.
    #[test]
    fn the_tree_is_well_formed_for_accesskit() {
        let dir = std::env::temp_dir().join(format!("nile-access-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("child.html"), "<body><button>Inside</button></body>").unwrap();
        let main = dir.join("main.html");
        std::fs::write(
            &main,
            r##"<html><head><title>T</title></head><body style="margin:0"><h1>Head</h1><p>Some <b>bold</b> text and a <a href="#">link</a>.</p>
               <label>Name <input value="x"></label><select><option>A</option><option selected>B</option></select>
               <ul><li>one</li><li>two</li></ul><table><tr><th>h</th><td>c</td></tr></table>
               <frame src="child.html" style="height:60px"></frame></body></html>"##,
        )
        .unwrap();
        let mut page = Page::load(main, (400.0, 300.0), 0.0);
        page.layout(400.0, 300.0);
        let tree = page.a11y_tree();
        assert!(tree.nodes.iter().any(|n| n.name == "Inside"), "the frame's document is part of the tree");
        let consumer = accesskit_consumer::Tree::new(update(&tree, 2.0), true);
        let state = consumer.state();
        assert_eq!(state.root().role(), Role::Window);
        assert!(state.focus().is_some());
        // Nothing is orphaned: the walk from the root finds every node we sent.
        fn count(n: accesskit_consumer::NodeRef<'_>) -> usize {
            1 + n.children().map(count).sum::<usize>()
        }
        assert_eq!(count(state.root()), tree.nodes.len());
        let _ = std::fs::remove_dir_all(dir);
    }
}

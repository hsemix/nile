//! CSS selector matching, via Servo's `selectors` crate.
//!
//! Supported: type, class, id, attribute selectors, all four combinators,
//! structural pseudo-classes (`:first-child`, `:nth-child()`, `:not()`,
//! `:is()`, `:root`, `:empty`, ...) and the interaction states `:hover`,
//! `:active`, `:focus` (driven by [`InteractionState`]). Rules using anything
//! else, such as pseudo-elements, are dropped whole, like a browser would.

use std::borrow::Borrow;
use std::fmt;

use cssparser::{match_ignore_ascii_case, CowRcStr, ParseError, ToCss};
use nile_dom::{Document, NodeId, NodeKind};
use precomputed_hash::PrecomputedHash;
use selectors::attr::{AttrSelectorOperation, CaseSensitivity, NamespaceConstraint};
use selectors::bloom::BloomFilter;
use selectors::context::MatchingContext;
use selectors::matching::ElementSelectorFlags;
use selectors::parser::{
    NonTSPseudoClass, Parser, PseudoElement as PseudoElementTrait, SelectorImpl,
    SelectorParseErrorKind,
};
use selectors::{Element, OpaqueElement};

/// The string type for every selector component.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Ident(pub String);

impl From<&str> for Ident {
    fn from(s: &str) -> Self {
        Ident(s.to_string())
    }
}

impl AsRef<str> for Ident {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for Ident {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl ToCss for Ident {
    fn to_css<W: fmt::Write>(&self, dest: &mut W) -> fmt::Result {
        cssparser::serialize_identifier(&self.0, dest)
    }
}

impl PrecomputedHash for Ident {
    fn precomputed_hash(&self) -> u32 {
        // FNV-1a.
        self.0.bytes().fold(0x811c_9dc5u32, |h, b| (h ^ u32::from(b)).wrapping_mul(0x0100_0193))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PseudoClass {
    Hover,
    Active,
    Focus,
    /// A runtime flag from [`nile_dom::STATE_NAMES`], e.g. `:checked`.
    State(&'static str),
}

impl ToCss for PseudoClass {
    fn to_css<W: fmt::Write>(&self, dest: &mut W) -> fmt::Result {
        dest.write_str(match self {
            PseudoClass::Hover => ":hover",
            PseudoClass::Active => ":active",
            PseudoClass::Focus => ":focus",
            PseudoClass::State(name) => {
                dest.write_char(':')?;
                return dest.write_str(name);
            }
        })
    }
}

impl NonTSPseudoClass for PseudoClass {
    fn is_active_or_hover(&self) -> bool {
        matches!(self, PseudoClass::Hover | PseudoClass::Active)
    }

    fn is_user_action_state(&self) -> bool {
        true
    }
}

/// The generated-content pseudo-elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PseudoElement {
    Before,
    After,
}

impl ToCss for PseudoElement {
    fn to_css<W: fmt::Write>(&self, dest: &mut W) -> fmt::Result {
        dest.write_str(match self {
            PseudoElement::Before => "::before",
            PseudoElement::After => "::after",
        })
    }
}

impl PseudoElementTrait for PseudoElement {}

#[derive(Debug, Clone, PartialEq)]
pub struct NileImpl;

impl SelectorImpl for NileImpl {
    type ExtraMatchingData<'a> = ();
    type AttrValue = Ident;
    type Identifier = Ident;
    type LocalName = Ident;
    type NamespaceUrl = Ident;
    type NamespacePrefix = Ident;
    type BorrowedNamespaceUrl = str;
    type BorrowedLocalName = str;
    type NonTSPseudoClass = PseudoClass;
    type PseudoElement = PseudoElement;
}

pub struct NileParser;

impl<'i> Parser<'i> for NileParser {
    type Impl = NileImpl;
    type Error = SelectorParseErrorKind;

    fn parse_is_and_where(&self) -> bool {
        true
    }

    fn parse_nth_child_of(&self) -> bool {
        true
    }

    fn parse_pseudo_element(
        &self,
        name: CowRcStr<'i>,
    ) -> Result<PseudoElement, ParseError<Self::Error>> {
        match_ignore_ascii_case! { &name,
            "before" => Ok(PseudoElement::Before),
            "after" => Ok(PseudoElement::After),
            _ => Err(ParseError::custom(SelectorParseErrorKind::UnsupportedPseudoClassOrElement)),
        }
    }

    fn parse_non_ts_pseudo_class(
        &self,
        name: CowRcStr<'i>,
    ) -> Result<PseudoClass, ParseError<Self::Error>> {
        match_ignore_ascii_case! { &name,
            "hover" => Ok(PseudoClass::Hover),
            "active" => Ok(PseudoClass::Active),
            "focus" => Ok(PseudoClass::Focus),
            _ => match nile_dom::STATE_NAMES.iter().find(|s| s.eq_ignore_ascii_case(&name)) {
                Some(state) => Ok(PseudoClass::State(state)),
                None => Err(ParseError::custom(SelectorParseErrorKind::UnsupportedPseudoClassOrElement)),
            },
        }
    }
}


/// Which elements are hovered, pressed or focused. The shell updates this from
/// input events and restyles; the default has nothing in any state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InteractionState {
    pub hover: Option<NodeId>,
    pub active: Option<NodeId>,
    pub focus: Option<NodeId>,
}

/// A DOM element as seen by the selector matcher.
#[derive(Clone)]
pub struct ElementRef<'a> {
    doc: &'a Document,
    id: NodeId,
    state: &'a InteractionState,
}

impl fmt::Debug for ElementRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<{}>", self.doc.node(self.id).tag().unwrap_or("?"))
    }
}

impl<'a> ElementRef<'a> {
    pub fn new(doc: &'a Document, id: NodeId, state: &'a InteractionState) -> Self {
        ElementRef { doc, id, state }
    }

    fn with(&self, id: NodeId) -> Self {
        ElementRef { id, ..self.clone() }
    }

    fn is_element(&self, id: NodeId) -> bool {
        matches!(self.doc.node(id).kind, NodeKind::Element { .. })
    }

    /// Element siblings of this node, in order.
    // TODO: sibling lookups scan the parent's child list, so wide lists are
    // quadratic. Cache element indices per node if this shows up in profiles.
    fn sibling_elements(&self) -> Vec<NodeId> {
        match self.doc.node(self.id).parent {
            Some(p) => self.doc.node(p).children.iter().copied().filter(|c| self.is_element(*c)).collect(),
            None => vec![self.id],
        }
    }

    /// True if `target` is this node or one of its descendants.
    fn contains(&self, target: Option<NodeId>) -> bool {
        let mut cur = target;
        while let Some(n) = cur {
            if n == self.id {
                return true;
            }
            cur = self.doc.node(n).parent;
        }
        false
    }
}

impl Element for ElementRef<'_> {
    type Impl = NileImpl;

    fn opaque(&self) -> OpaqueElement {
        OpaqueElement::new(self.doc.node(self.id))
    }

    fn parent_element(&self) -> Option<Self> {
        let p = self.doc.node(self.id).parent?;
        self.is_element(p).then(|| self.with(p))
    }

    fn parent_node_is_shadow_root(&self) -> bool {
        false
    }

    fn containing_shadow_host(&self) -> Option<Self> {
        None
    }

    fn is_pseudo_element(&self) -> bool {
        false
    }

    fn prev_sibling_element(&self) -> Option<Self> {
        let sibs = self.sibling_elements();
        let i = sibs.iter().position(|s| *s == self.id)?;
        i.checked_sub(1).map(|j| self.with(sibs[j]))
    }

    fn next_sibling_element(&self) -> Option<Self> {
        let sibs = self.sibling_elements();
        let i = sibs.iter().position(|s| *s == self.id)?;
        sibs.get(i + 1).map(|s| self.with(*s))
    }

    fn first_element_child(&self) -> Option<Self> {
        self.doc.node(self.id).children.iter().find(|c| self.is_element(**c)).map(|c| self.with(*c))
    }

    fn is_html_element_in_html_document(&self) -> bool {
        true
    }

    fn has_local_name(&self, name: &str) -> bool {
        self.doc.node(self.id).tag().is_some_and(|t| t.eq_ignore_ascii_case(name))
    }

    fn has_namespace(&self, _ns: &str) -> bool {
        true
    }

    fn is_same_type(&self, other: &Self) -> bool {
        self.doc.node(self.id).tag() == other.doc.node(other.id).tag()
    }

    fn attr_matches(
        &self,
        _ns: &NamespaceConstraint<&Ident>,
        local_name: &Ident,
        operation: &AttrSelectorOperation<&Ident>,
    ) -> bool {
        let node = self.doc.node(self.id);
        match &node.kind {
            NodeKind::Element { attrs, .. } => attrs
                .iter()
                .any(|(k, v)| k.eq_ignore_ascii_case(&local_name.0) && operation.eval_str(v)),
            _ => false,
        }
    }

    fn match_non_ts_pseudo_class(
        &self,
        pc: &PseudoClass,
        _context: &mut MatchingContext<NileImpl>,
    ) -> bool {
        match pc {
            PseudoClass::Hover => self.contains(self.state.hover),
            PseudoClass::Active => self.contains(self.state.active),
            PseudoClass::Focus => self.state.focus == Some(self.id),
            PseudoClass::State(name) => self.doc.has_state(self.id, name),
        }
    }

    // Normal matching never selects the element itself for a pseudo-element selector;
    // the cascade matches those separately in `ForStatelessPseudoElement` mode.
    fn match_pseudo_element(&self, _: &PseudoElement, _: &mut MatchingContext<NileImpl>) -> bool {
        false
    }

    fn apply_selector_flags(&self, _flags: ElementSelectorFlags) {}

    fn is_link(&self) -> bool {
        let n = self.doc.node(self.id);
        matches!(n.tag(), Some("a" | "area")) && n.attr("href").is_some()
    }

    fn is_html_slot_element(&self) -> bool {
        false
    }

    fn has_id(&self, id: &Ident, case: CaseSensitivity) -> bool {
        self.doc.node(self.id).attr("id").is_some_and(|v| case.eq(v.as_bytes(), id.0.as_bytes()))
    }

    fn has_class(&self, name: &Ident, case: CaseSensitivity) -> bool {
        self.doc
            .node(self.id)
            .attr("class")
            .is_some_and(|c| c.split_whitespace().any(|w| case.eq(w.as_bytes(), name.0.as_bytes())))
    }

    fn has_custom_state(&self, _name: &Ident) -> bool {
        false
    }

    fn imported_part(&self, _name: &Ident) -> Option<Ident> {
        None
    }

    fn is_part(&self, _name: &Ident) -> bool {
        false
    }

    fn is_empty(&self) -> bool {
        self.doc.node(self.id).children.iter().all(|c| match &self.doc.node(*c).kind {
            NodeKind::Text(t) => t.trim().is_empty(),
            _ => false,
        })
    }

    fn is_root(&self) -> bool {
        self.doc.node(self.id).parent == Some(Document::ROOT) && self.doc.node(self.id).tag() == Some("html")
    }

    fn add_element_unique_hashes(&self, _filter: &mut BloomFilter) -> bool {
        false
    }
}

/// A parsed selector list for `querySelector`, `matches` and friends.
pub struct Query(selectors::SelectorList<NileImpl>);

impl Query {
    /// Parse `css`; the error is a short human-readable message.
    pub fn parse(css: &str) -> Result<Query, String> {
        let mut parser = cssparser::Parser::new(css);
        selectors::SelectorList::parse(&NileParser, &mut parser, selectors::parser::ParseRelative::No)
            .map(Query)
            .map_err(|_| format!("'{css}' is not a valid selector"))
    }

    pub fn matches(&self, doc: &Document, id: NodeId) -> bool {
        self.matches_with(doc, id, &InteractionState::default())
    }

    /// Like [`Query::matches`], with `:hover`/`:active`/`:focus` resolved against `state`.
    pub fn matches_with(&self, doc: &Document, id: NodeId, state: &InteractionState) -> bool {
        use selectors::context::{
            MatchingForInvalidation, MatchingMode, NeedsSelectorFlags, QuirksMode, SelectorCaches,
        };
        let el = ElementRef::new(doc, id, state);
        let mut caches = SelectorCaches::default();
        let mut ctx = MatchingContext::new(
            MatchingMode::Normal,
            None,
            &mut caches,
            QuirksMode::NoQuirks,
            NeedsSelectorFlags::No,
            MatchingForInvalidation::No,
        );
        selectors::matching::matches_selector_list(&self.0, &el, &mut ctx)
    }

    /// Descendants of `root` (not `root` itself) that match, in document order.
    pub fn select_all(&self, doc: &Document, root: NodeId) -> Vec<NodeId> {
        self.select_all_with(doc, root, &InteractionState::default())
    }

    pub fn select_all_with(&self, doc: &Document, root: NodeId, state: &InteractionState) -> Vec<NodeId> {
        doc.descendants(root)
            .into_iter()
            .filter(|id| matches!(doc.node(*id).kind, NodeKind::Element { .. }) && self.matches_with(doc, *id, state))
            .collect()
    }
}

//! Reading a file's form (M10 proposal 8): the field tree with inherited
//! keys resolved, each field's widgets on pages, and the facts sheaf shows
//! about the form: its class, scripts, usage rights and signatures
//! (proposal 19).
//!
//! It's read from the source, lazily: only the objects the form reaches
//! are parsed, and the widgets are found through the page table's
//! `/Annots`, so a page the tree lists twice has its widgets at both of
//! its indexes.
//!
//! The tree comes from untrusted files and is read for every file that
//! opens, so it's read within the source's [`FormLimits`]: a visited set
//! on `/Kids` (a cycle is damage; a field or widget listed twice is read
//! where it's reached first), and by default at most 32 deep, 100,000
//! nodes, 200,000 entries in `/Fields` and the `/Kids` arrays together (so
//! many fields sharing kids can't make work), 100,000 widgets on pages,
//! full names of at most 64 KiB each and 64 MiB in all, `/Opt` and list
//! values of at most 10,000 items, and values, scripts and the rest of the
//! text of at most 1 MiB each and 64 MiB in all. Past any of them the form
//! is damaged: none of its fields are kept, so nothing can be filled, and
//! the file still opens. Rects, widths, dashes and colors are made finite
//! and clamped, each number followed where it's a reference. `/MaxLen` is
//! kept as the file has it; layout uses at most 1,000 of it. `/TI` is read
//! as the family reads it: an integer within `u32`, else 0.
//!
//! Fields are read down `/Kids` from `/Fields`, as PDFium reads them;
//! `/Parent` isn't followed (the form check reports kids whose `/Parent`
//! is elsewhere), so a loop through `/Parent` alone isn't damage. A field `/Fields` lists that's also another's kid is
//! read as the kid, under its parent's name. Fields and kids that are
//! direct objects aren't read. A full name joins the `/T`s from the root
//! down, as PDFium's does, passing over a node with none or an empty one.
//! Values, options and actions many fields share are read once, so a
//! value a thousand fields inherit is one string. An `/Opt` is read once
//! for both of the ways a field takes it: a choice field's items, which
//! leave out an item that is no text, and a check box's or radio group's
//! export values, which keep its place.
//!
//! Every node of the tree is numbered, in tree order (M10.5 §4): a
//! terminal field's id is its node's.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use crate::pdf::{
    Dictionary, LazyPdf, Limit, Object, ObjectId, StreamExt, Value as PdfValue, text,
};

use super::field::{
    Action, Border, BorderStyle, Choice, Color, Field, FieldId, FieldKind, Mk, Trigger, Value,
    Widget,
};
use crate::attachments::{Objects, once, resolve};
use crate::source::{FormLimits, PageFacts, Source};

/// The most of `/MaxLen` layout uses: comb cells and character limits.
pub const MAX_LAYOUT_LEN: u32 = 1000;
/// How far from the origin a widget's rect may reach, in points.
pub const MAX_COORD: f32 = 1_000_000.0;

/// A file's form as read when it opens: what its source says about it
/// ([`Source::form`]), which the open and the pages that come in from it
/// read.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FileForm {
    pub facts: FormFacts,
    /// Every node of the field tree, in tree order, node `i` with id `i`.
    /// None when the form is damaged.
    pub nodes: Vec<Node>,
    /// Terminal fields, in tree order, each with its node's id and source
    /// 0. None when the form is damaged.
    pub fields: Vec<Field>,
    /// Their widgets on pages, by page index, each page's in `/Annots`
    /// order. A widget on two pages is on both.
    pub widgets: BTreeMap<u32, Vec<Widget>>,
    /// Where each terminal field is among `fields`, by its object
    /// ([`field_by_object`](Self::field_by_object)).
    by_object: HashMap<ObjectId, usize>,
    /// Where each widget is in `widgets`, by its object: the page and its
    /// place among the page's, the first page's of a widget on two
    /// ([`widget_by_object`](Self::widget_by_object)).
    widget_at: HashMap<ObjectId, (u32, usize)>,
}

impl FileForm {
    /// The terminal field whose dictionary is object `id`, as read: what a
    /// field of the model's was read as, found without a scan.
    pub fn field_by_object(&self, id: ObjectId) -> Option<&Field> {
        self.fields.get(*self.by_object.get(&id)?)
    }

    /// The widget whose dictionary is object `id`, as read: what a widget
    /// of the model's was read as, found without a scan. A widget on two
    /// pages is read on each alike; this is the first page's.
    pub fn widget_by_object(&self, id: ObjectId) -> Option<&Widget> {
        let &(page, at) = self.widget_at.get(&id)?;
        self.widgets.get(&page)?.get(at)
    }
}

/// A node of a form's field tree: a terminal field, or a parent of
/// others.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub object: ObjectId,
    /// `/T`: empty when it has none.
    pub partial: Arc<str>,
    pub parent: Option<FieldId>,
    /// The fields under it, in `/Kids` order; not its widgets.
    pub kids: Vec<FieldId>,
}

/// What sheaf shows about a form: in Properties, the XFA-only bar, and the
/// notice for a damaged one.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FormFacts {
    pub class: FormClass,
    /// Terminal fields, and how many of each kind.
    pub fields: usize,
    pub kinds: BTreeMap<FieldKind, usize>,
    /// Widgets on pages.
    pub widgets: usize,
    /// Fields with scripts, by trigger.
    pub scripts: ScriptCounts,
    /// Document-level scripts (`/Names /JavaScript`), besides Adobe's
    /// version checks (`!ADBE::`), which never run (proposal 13).
    pub document_scripts: usize,
    pub version_checks: usize,
    /// `/AcroForm /XFA`, and the catalog's `/NeedsRendering`.
    pub xfa: bool,
    /// Fields whose value a static XFA copy holds and the AcroForm doesn't
    /// (proposal 6): sheaf shows the AcroForm's, so these aren't shown.
    pub xfa_values: usize,
    pub dynamic: bool,
    pub need_appearances: bool,
    /// `/AcroForm /SigFlags`.
    pub sig_flags: i64,
    /// `/Perms /UR3` (or `/UR`): Reader's usage rights.
    pub usage_rights: bool,
    /// `/Perms /DocMDP`: a certification.
    pub certified: bool,
    pub signature_fields: usize,
    /// Signature fields holding a signature.
    pub signed: usize,
    /// The form's `/DA` and `/Q`, for fields that set neither.
    pub da: Option<Arc<str>>,
    pub align: u8,
    /// The fonts `/DR /Font` names.
    pub fonts: Vec<String>,
    /// `/CO`: the fields that calculate, in order.
    pub calculation_order: Vec<FieldId>,
    /// Why the field tree can't be read: then the form isn't fillable, and
    /// the counts are what was read before it.
    pub damaged: Option<String>,
}

/// What a file's form is (proposal 19).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FormClass {
    /// No fields, and no XFA.
    #[default]
    None,
    AcroForm,
    /// An AcroForm with an XFA copy beside it, filled through the AcroForm
    /// (proposal 6).
    Hybrid,
    /// The XFA is the form: dynamic (`/NeedsRendering`), or with no
    /// AcroForm fields. sheaf shows the file's own pages and can't fill it.
    XfaOnly,
}

impl FormClass {
    pub fn of(xfa: bool, dynamic: bool, fields: usize) -> FormClass {
        match (xfa, fields) {
            (true, 0) => FormClass::XfaOnly,
            (true, _) if dynamic => FormClass::XfaOnly,
            (true, _) => FormClass::Hybrid,
            (false, 0) => FormClass::None,
            (false, _) => FormClass::AcroForm,
        }
    }

    /// "AcroForm", "hybrid", "XFA-only" or "no form".
    pub fn label(self) -> &'static str {
        match self {
            FormClass::None => "no form",
            FormClass::AcroForm => "AcroForm",
            FormClass::Hybrid => "hybrid",
            FormClass::XfaOnly => "XFA-only",
        }
    }
}

/// Fields with a script for each of their own triggers, and with any
/// script at all, their own or on a widget (focus, blur, the mouse, a
/// button's action).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScriptCounts {
    pub keystroke: usize,
    pub format: usize,
    pub validate: usize,
    pub calculate: usize,
    pub fields: usize,
}

impl FormFacts {
    /// Field or document scripts that would run.
    pub fn has_scripts(&self) -> bool {
        self.scripts.fields > 0 || self.document_scripts > 0
    }

    pub fn is_fillable(&self) -> bool {
        self.damaged.is_none() && matches!(self.class, FormClass::AcroForm | FormClass::Hybrid)
    }
}

impl fmt::Display for FormFacts {
    /// One line for Properties: the class, then fields, scripts, usage
    /// rights and signatures.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(why) = &self.damaged {
            return write!(f, "Damaged ({why}), so not fillable");
        }
        let class = match self.class {
            FormClass::None => return f.write_str("None"),
            FormClass::AcroForm => "AcroForm",
            FormClass::Hybrid => "Hybrid (AcroForm and XFA)",
            FormClass::XfaOnly => return f.write_str("XFA only, which sheaf can't fill"),
        };
        let mut parts = vec![class.to_string(), count(self.fields, "field")];
        parts.push(match (self.scripts.fields, self.document_scripts) {
            (0, 0) => "no scripts".into(),
            (0, d) => count(d, "document script"),
            (n, 0) => format!("scripts on {}", count(n, "field")),
            (n, d) => format!("scripts on {} and {d} at document level", count(n, "field")),
        });
        if self.usage_rights {
            parts.push("Reader usage rights".into());
        }
        if self.certified {
            parts.push("certified".into());
        }
        if self.signature_fields > 0 {
            parts.push(format!(
                "{}, {} signed",
                count(self.signature_fields, "signature field"),
                self.signed
            ));
        }
        f.write_str(&parts.join("; "))
    }
}

fn count(n: usize, what: &str) -> String {
    if n == 1 {
        format!("1 {what}")
    } else {
        format!("{n} {what}s")
    }
}

/// Read the form in `src`, within its [`FormLimits`].
pub fn read(src: &Source) -> FileForm {
    let pdf = src.pdf();
    let limits = &src.limits().forms;
    let catalog = pdf.catalog().ok();
    let form = catalog
        .and_then(|c| c.get(b"AcroForm"))
        .and_then(|f| resolve(pdf, f))
        .and_then(Object::as_dict);
    let mut r = Reader::new(pdf, limits);
    let mut facts = FormFacts::default();
    if let Some(form) = form {
        facts.xfa = form
            .get(b"XFA")
            .and_then(|x| resolve(pdf, x))
            .is_some_and(|x| matches!(x.value(), PdfValue::Array(_) | PdfValue::Stream(_)));
        facts.need_appearances = boolean(pdf, form, b"NeedAppearances");
        facts.sig_flags = number(pdf, form, b"SigFlags").unwrap_or(0);
        facts.da = r.da(form);
        facts.align = align(pdf, form).unwrap_or(0);
        let fonts = form
            .get(b"DR")
            .and_then(|d| resolve(pdf, d))
            .and_then(Object::as_dict)
            .and_then(|d| d.get(b"Font"))
            .and_then(|f| resolve(pdf, f))
            .and_then(Object::as_dict);
        if let Some(fonts) = fonts {
            for (name, _) in fonts.iter() {
                if r.spend(name.len()) {
                    facts.fonts.push(latin1(name));
                }
            }
        }
        r.tree(form);
    }
    facts.dynamic = catalog.is_some_and(|c| boolean(pdf, c, b"NeedsRendering"));
    let perms = catalog
        .and_then(|c| c.get(b"Perms"))
        .and_then(|p| resolve(pdf, p))
        .and_then(Object::as_dict);
    let perm = |key: &[u8]| {
        perms
            .and_then(|p| p.get(key))
            .and_then(|s| resolve(pdf, s))
            .is_some_and(|s| matches!(s.value(), PdfValue::Dict(_)))
    };
    facts.usage_rights = perm(b"UR3") || perm(b"UR");
    facts.certified = perm(b"DocMDP");
    (facts.document_scripts, facts.version_checks) = document_scripts(pdf, limits);

    let widgets = r.on_pages(src.pages());
    let fields = std::mem::take(&mut r.fields);
    let nodes = std::mem::take(&mut r.nodes);
    // A damaged form has a field tree, however little of it was read: a
    // damaged hybrid form isn't XFA-only.
    let read = match r.damaged {
        Some(_) => fields.len().max(1),
        None => fields.len(),
    };
    facts.class = FormClass::of(facts.xfa, facts.dynamic, read);
    facts.fields = fields.len();
    facts.widgets = widgets.values().map(Vec::len).sum();
    let mut scripted: HashSet<FieldId> = widgets
        .values()
        .flatten()
        .filter(|w| w.actions.iter().any(|(_, a)| a.has_script()))
        .map(|w| w.field)
        .collect();
    for f in &fields {
        *facts.kinds.entry(f.kind).or_default() += 1;
        let has = |t: Trigger| usize::from(f.script(t).is_some());
        facts.scripts.keystroke += has(Trigger::Keystroke);
        facts.scripts.format += has(Trigger::Format);
        facts.scripts.validate += has(Trigger::Validate);
        facts.scripts.calculate += has(Trigger::Calculate);
        if f.actions.iter().any(|(_, a)| a.has_script()) {
            scripted.insert(f.id);
        }
        if f.kind == FieldKind::Signature {
            facts.signature_fields += 1;
            facts.signed += usize::from(f.value == Some(Value::Signed));
        }
    }
    facts.scripts.fields = scripted.len();
    let by_object: HashMap<ObjectId, FieldId> = fields.iter().map(|f| (f.object, f.id)).collect();
    let order = form
        .and_then(|f| f.get(b"CO"))
        .and_then(|c| resolve(pdf, c))
        .and_then(Object::as_array);
    facts.calculation_order = order
        .into_iter()
        .flatten()
        .take(limits.nodes)
        .filter_map(|o| by_object.get(&o.as_reference()?).copied())
        .collect();

    if let Some(why) = r.damaged {
        facts.damaged = Some(why);
        return FileForm {
            facts,
            ..Default::default()
        };
    }
    let mut fields = fields;
    // A static XFA copy: the values it holds that the AcroForm lacks, and
    // its tooltips where the AcroForm has none.
    if facts.xfa
        && !facts.dynamic
        && let Some(form) = form
    {
        let xfa = super::xfa::Lookup::new(super::xfa::read(pdf, form, limits));
        for f in &mut fields {
            let Some(x) = xfa.get(&f.name) else {
                continue;
            };
            if x.value.as_deref().is_some_and(|v| lacks(f, v)) {
                facts.xfa_values += 1;
            }
            if f.tooltip.is_none() {
                f.tooltip = x.tooltip.clone();
            }
        }
    }
    // A field's object is its node's, and the tree reads a node once.
    let by_object = (fields.iter().enumerate())
        .map(|(i, f)| (f.object, i))
        .collect();
    let mut widget_at = HashMap::new();
    for (&page, list) in &widgets {
        for (at, w) in list.iter().enumerate() {
            widget_at.entry(w.object).or_insert((page, at));
        }
    }
    FileForm {
        facts,
        nodes,
        fields,
        widgets,
        by_object,
        widget_at,
    }
}

/// Whether field `f` lacks `value`, which its XFA copy holds: text it
/// doesn't have, or a box checked there and not here. A check box's XFA
/// value is its state's number: `0` is off.
fn lacks(f: &Field, value: &str) -> bool {
    match f.kind {
        FieldKind::CheckBox | FieldKind::Radio => {
            !matches!(value, "0" | "Off" | "off")
                && !matches!(&f.value, Some(Value::State(s)) if &**s != "Off")
        }
        FieldKind::Text | FieldKind::Combo | FieldKind::List => match &f.value {
            None => true,
            Some(Value::Text(t)) => t.is_empty(),
            Some(Value::Items(items)) => items.is_empty(),
            Some(_) => false,
        },
        _ => false,
    }
}

/// What a field passes down to its kids.
#[derive(Clone, Default)]
struct Inherited<'a> {
    ft: &'a [u8],
    flags: u32,
    value: Option<Value>,
    default: Option<Value>,
    da: Option<Arc<str>>,
    align: Option<u8>,
    max_len: Option<u32>,
    top: Option<u32>,
    options: Option<Opt>,
}

/// An `/Opt` as read, once, in both of the ways a field takes it.
#[derive(Clone)]
struct Opt {
    /// It is an array, an empty one too.
    array: bool,
    /// A choice field's items: each item that is text, or a pair of text.
    items: Arc<[Choice]>,
    /// A check box's or radio group's export values: every item in its
    /// place, as the family reads an option: a string, or an array of
    /// exactly two strings; anything else an option of empty text.
    /// `items` itself where every item is one of those.
    exports: Arc<[Choice]>,
}

impl Opt {
    /// What a field of `kind` takes of it.
    fn for_kind(&self, kind: FieldKind) -> Arc<[Choice]> {
        match kind {
            FieldKind::CheckBox | FieldKind::Radio => self.exports.clone(),
            _ => self.items.clone(),
        }
    }
}

/// Where a widget of the field tree belongs.
#[derive(Clone, Copy)]
struct WidgetOf {
    /// Its field, by its index in `fields`.
    field: usize,
    /// It's a kid rather than the field's own dictionary.
    kid: bool,
    /// Its place among its field's widgets ([`Widget::index`]).
    index: u32,
}

struct Reader<'a, P: Objects + ?Sized> {
    pdf: &'a P,
    limits: FormLimits,
    nodes: Vec<Node>,
    fields: Vec<Field>,
    /// Each widget's field, and its place among the field's.
    widget_of: HashMap<ObjectId, WidgetOf>,
    seen: HashSet<ObjectId>,
    /// The fields from the root down to the one being read.
    path: HashSet<ObjectId>,
    /// Fields reached, read or not.
    visited: usize,
    /// Bytes of full names, and of other text, so far.
    names: usize,
    text: usize,
    damaged: Option<String>,
    /// What's been read from objects many fields can share.
    values: HashMap<ObjectId, Option<Value>>,
    options: HashMap<ObjectId, Opt>,
    actions: HashMap<ObjectId, Option<Action>>,
}

impl<'a, P: Objects + ?Sized> Reader<'a, P> {
    fn new(pdf: &'a P, limits: &FormLimits) -> Self {
        Reader {
            pdf,
            limits: *limits,
            nodes: Vec::new(),
            fields: Vec::new(),
            widget_of: HashMap::new(),
            seen: HashSet::new(),
            path: HashSet::new(),
            visited: 0,
            names: 0,
            text: 0,
            damaged: None,
            values: HashMap::new(),
            options: HashMap::new(),
            actions: HashMap::new(),
        }
    }

    fn damage(&mut self, why: String) {
        self.damaged.get_or_insert(why);
    }

    /// Count `bytes` more of text against [`FormLimits::texts`]; false
    /// (and the form damaged) past it.
    fn spend(&mut self, bytes: usize) -> bool {
        self.text += bytes;
        if self.text > self.limits.texts {
            self.damage(format!(
                "its text comes to more than {} MiB",
                self.limits.texts >> 20
            ));
        }
        self.damaged.is_none()
    }

    /// The field tree from `form`'s `/Fields`.
    fn tree(&mut self, form: &Dictionary) {
        let pdf = self.pdf;
        let mut roots: Vec<ObjectId> = form
            .get(b"Fields")
            .and_then(|f| resolve(pdf, f))
            .and_then(Object::as_array)
            .map(|f| f.iter().filter_map(Object::as_reference).collect())
            .unwrap_or_default();
        match listed_as_kids(pdf, &roots, self.limits.entries) {
            Some(kids) => {
                // Roots that are kids too come last, so their parents read
                // them first.
                roots.sort_by_key(|id| kids.contains(id));
                for root in roots {
                    self.node(root, Inherited::default(), "", None, 1);
                }
            }
            None => self.damage(format!(
                "/Fields and /Kids list more than {} entries",
                self.limits.entries
            )),
        }
    }

    /// Read field `id`, a kid of node `parent` (none for a root) with full
    /// name `up_name`.
    fn node(
        &mut self,
        id: ObjectId,
        up: Inherited<'a>,
        up_name: &str,
        parent: Option<FieldId>,
        depth: usize,
    ) {
        if self.damaged.is_some() {
            return;
        }
        let limits = self.limits;
        if depth > limits.depth {
            return self.damage(format!("the field tree is more than {} deep", limits.depth));
        }
        if self.path.contains(&id) {
            return self.damage(cycle());
        }
        // Listed twice, it's read once, where it's reached first.
        if !self.seen.insert(id) {
            return;
        }
        self.visited += 1;
        if self.visited > limits.nodes {
            return self.damage(format!(
                "the field tree has more than {} nodes",
                limits.nodes
            ));
        }
        let pdf = self.pdf;
        let Some(dict) = pdf.dictionary(id) else {
            return;
        };
        let part = dict.get(b"T").and_then(|t| resolve(pdf, t));
        let partial = match part.and_then(Object::as_bytes) {
            Some(t) if t.len() > limits.name => None,
            Some(t) => Some(text::decode(t)),
            None => Some(String::new()),
        };
        let name = partial.as_deref().map(|t| super::full_name_of(up_name, t));
        let (Some(partial), Some(name)) = (partial, name.filter(|n| n.len() <= limits.name)) else {
            return self.damage(format!(
                "a full name is longer than {} KiB",
                limits.name >> 10
            ));
        };
        self.names += name.len();
        if self.names > limits.names {
            return self.damage(format!(
                "the full names come to more than {} MiB",
                limits.names >> 20
            ));
        }
        let me = FieldId(self.nodes.len() as u64);
        self.nodes.push(Node {
            object: id,
            partial: partial.into(),
            parent,
            kids: Vec::new(),
        });
        if let Some(p) = parent {
            self.nodes[p.0 as usize].kids.push(me);
        }

        let mut here = up;
        if let Some(ft) = dict.get(b"FT").and_then(Object::as_name) {
            here.ft = ft;
        }
        if let Some(ff) = number(pdf, dict, b"Ff") {
            here.flags = ff as u32;
        }
        if let Some(v) = dict.get(b"V") {
            here.value = self.value(v);
        }
        if let Some(v) = dict.get(b"DV") {
            here.default = self.value(v);
        }
        if let Some(da) = self.da(dict) {
            here.da = Some(da);
        }
        if let Some(q) = align(pdf, dict) {
            here.align = Some(q);
        }
        if let Some(n) = number(pdf, dict, b"MaxLen") {
            here.max_len = u32::try_from(n.max(0)).ok().or(Some(u32::MAX));
        }
        // An integer past `u32`, less than 0 too, is 0, as the family reads
        // one (`Field::top_index`).
        if let Some(n) = number(pdf, dict, b"TI") {
            here.top = Some(u32::try_from(n).unwrap_or(0));
        }
        if let Some(opt) = dict.get(b"Opt") {
            here.options = Some(self.choices(opt));
        }
        if self.damaged.is_some() {
            return;
        }

        // `listed_as_kids` has counted these against `limits.entries`.
        let kids = dict
            .get(b"Kids")
            .and_then(|k| resolve(pdf, k))
            .and_then(Object::as_array)
            .unwrap_or_default();
        let mut widgets = Vec::new();
        let mut field_kids = false;
        self.path.insert(id);
        for kid in kids.iter().filter_map(Object::as_reference) {
            let Some(k) = pdf.dictionary(kid) else {
                continue;
            };
            // A kid with a name is a field; one without is a widget,
            // unless it has kids of its own.
            if k.has(b"T") || (!is_widget(k) && k.has(b"Kids")) {
                field_kids = true;
                self.node(kid, here.clone(), &name, Some(me), depth + 1);
            } else if self.path.contains(&kid) {
                self.damage(cycle());
            } else if self.seen.insert(kid) {
                widgets.push((kid, true));
            }
            if self.damaged.is_some() {
                return;
            }
        }
        self.path.remove(&id);
        // The field's own dictionary, when it's a widget too, after its
        // kids that are: PDFium counts a field's widgets as the family
        // does, its kids, or itself where it has none, so each kid's place
        // among them is theirs.
        if is_widget(dict) {
            widgets.push((id, false));
        }
        if widgets.is_empty() && field_kids {
            return;
        }
        let kind = FieldKind::of(here.ft, here.flags);
        // A check box or radio field's widgets' on states, every one's,
        // a page's or not: its group names each by it.
        let mut on_states = Vec::new();
        if matches!(kind, FieldKind::CheckBox | FieldKind::Radio) {
            for &(w, _) in &widgets {
                let states = (pdf.dictionary(w))
                    .and_then(|d| normal_of(pdf, d))
                    .and_then(Object::as_dict);
                let on = states.and_then(|n| on_state_of(pdf, n));
                on_states.push(on.filter(|on| self.spend(on.len())));
            }
            if self.damaged.is_some() {
                return;
            }
        }
        let tooltip = match dict.get(b"TU").and_then(|t| resolve(pdf, t)) {
            Some(t) => self.text(t),
            None => None,
        };
        let actions = self.actions(dict, false);
        if self.damaged.is_some() {
            return;
        }
        let field = self.fields.len();
        for ((w, kid), index) in widgets.into_iter().zip(0..) {
            self.widget_of.insert(w, WidgetOf { field, kid, index });
        }
        let has_opt = here.options.as_ref().is_some_and(|o| o.array);
        let options = here.options.map(|o| o.for_kind(kind));
        self.fields.push(Field {
            id: me,
            object: id,
            name,
            kind,
            flags: here.flags,
            value: here.value.map(|v| fit(v, kind)),
            default: here.default.map(|v| fit(v, kind)),
            da: here.da,
            align: here.align,
            max_len: here.max_len,
            top: here.top.unwrap_or(0),
            options: options.unwrap_or_else(|| Arc::from([])),
            has_opt,
            on_states,
            tooltip: tooltip.map(|t| t.to_string()),
            actions,
            outside: 0,
        });
    }

    /// A `/V` or `/DV`: text, a state, items, or a signature.
    fn value(&mut self, v: &Object) -> Option<Value> {
        if let Some(id) = v.as_reference()
            && let Some(value) = self.values.get(&id)
        {
            return value.clone();
        }
        let value = resolve(self.pdf, v).and_then(|o| match o.value() {
            PdfValue::Name(n) => self
                .spend(n.as_bytes().len())
                .then(|| Value::State(latin1(n.as_bytes()).into())),
            PdfValue::Array(items) => {
                if items.len() > self.limits.items {
                    let n = self.limits.items;
                    self.damage(format!("a value has more than {n} items"));
                    None
                } else {
                    let items: Vec<Arc<str>> =
                        items.iter().filter_map(|item| self.text(item)).collect();
                    Some(Value::Items(items.into()))
                }
            }
            PdfValue::Dict(d) => d.has(b"ByteRange").then_some(Value::Signed),
            _ => self.text(o).map(Value::Text),
        });
        if let Some(id) = v.as_reference() {
            self.values.insert(id, value.clone());
        }
        value
    }

    /// `/Opt`: each item a string, or an export value and what's shown;
    /// read once for a choice field and for a button field ([`Opt`]).
    fn choices(&mut self, opt: &Object) -> Opt {
        if let Some(id) = opt.as_reference()
            && let Some(choices) = self.options.get(&id)
        {
            return choices.clone();
        }
        let array = resolve(self.pdf, opt).and_then(Object::as_array);
        let items = array.unwrap_or_default();
        let mut choices = Vec::new();
        // The button field's reading, once an item isn't text.
        let mut exports: Option<Vec<Choice>> = None;
        if items.len() > self.limits.items {
            let n = self.limits.items;
            self.damage(format!("an /Opt has more than {n} items"));
        } else {
            let pdf = self.pdf;
            let is_string = |o: &Object| {
                resolve(pdf, o).is_some_and(|o| matches!(o.value(), PdfValue::String(..)))
            };
            for item in items {
                let pair = resolve(pdf, item).and_then(Object::as_array);
                let (export, shown) = match pair {
                    Some([export, shown, ..]) => (self.text(export), self.text(shown)),
                    Some(_) => (None, None),
                    None => {
                        let text = self.text(item);
                        (text.clone(), text)
                    }
                };
                let choice = match (export, shown) {
                    (Some(export), Some(shown)) => Some(Choice { export, shown }),
                    _ => None,
                };
                // A button field's, as the family reads an option: a
                // string, or an array of exactly two strings; anything
                // else (a longer array, a stream's text) an empty one.
                let strict = match pair {
                    Some([export, shown]) => is_string(export) && is_string(shown),
                    Some(_) => false,
                    None => is_string(item),
                };
                let button = choice.clone().filter(|_| strict);
                if button.is_none() || exports.is_some() {
                    let empty = || {
                        let empty: Arc<str> = Arc::from("");
                        Choice {
                            export: empty.clone(),
                            shown: empty,
                        }
                    };
                    (exports.get_or_insert_with(|| choices.clone()))
                        .push(button.unwrap_or_else(empty));
                }
                choices.extend(choice);
            }
        }
        let items: Arc<[Choice]> = choices.into();
        let read = Opt {
            array: array.is_some(),
            exports: exports.map_or_else(|| items.clone(), Arc::from),
            items,
        };
        if let Some(id) = opt.as_reference() {
            self.options.insert(id, read.clone());
        }
        read
    }

    /// A string's or stream's text, at most [`FormLimits::text`] bytes of
    /// it.
    fn text(&mut self, o: &Object) -> Option<Arc<str>> {
        let max = self.limits.text;
        let too_long = || format!("a value or script is longer than {} MiB", max >> 20);
        let bytes = match resolve(self.pdf, o)?.value() {
            PdfValue::String(s, _) => std::borrow::Cow::Borrowed(s.as_bytes()?),
            PdfValue::Stream(s) => match s.decompressed_content_with_limit(max) {
                Ok(c) => std::borrow::Cow::Owned(c),
                Err(crate::pdf::Error::Limit(Limit::Decompressed)) => {
                    self.damage(too_long());
                    return None;
                }
                Err(_) => return None,
            },
            _ => return None,
        };
        if bytes.len() > max {
            self.damage(too_long());
            return None;
        }
        self.spend(bytes.len()).then(|| text::decode(&bytes).into())
    }

    /// A dictionary's `/DA`, unless it's longer than [`FormLimits::da`].
    fn da(&mut self, d: &Dictionary) -> Option<Arc<str>> {
        let da = resolve(self.pdf, d.get(b"DA")?)?.as_bytes()?;
        (da.len() <= self.limits.da && self.spend(da.len())).then(|| latin1(da).into())
    }

    /// A field's actions in `d`'s `/AA`; or a widget's, and its `/A`.
    fn actions(&mut self, d: &Dictionary, widget: bool) -> Vec<(Trigger, Action)> {
        let pdf = self.pdf;
        let mut out = Vec::new();
        let triggers: &[(&[u8], Trigger)] = if widget {
            &Trigger::WIDGET
        } else {
            &Trigger::FIELD
        };
        if widget
            && let Some(a) = d.get(b"A")
            && let Some(action) = self.action(a)
        {
            out.push((Trigger::Activate, action));
        }
        let aa = d
            .get(b"AA")
            .and_then(|a| resolve(pdf, a))
            .and_then(Object::as_dict);
        for &(key, trigger) in triggers {
            if let Some(a) = aa.and_then(|aa| aa.get(key))
                && let Some(action) = self.action(a)
            {
                out.push((trigger, action));
            }
        }
        out
    }

    /// An action: its `/S`, and a JavaScript action's `/JS`. (`/Next` isn't
    /// followed.)
    fn action(&mut self, a: &Object) -> Option<Action> {
        if let Some(id) = a.as_reference()
            && let Some(action) = self.actions.get(&id)
        {
            return action.clone();
        }
        let d = resolve(self.pdf, a).and_then(Object::as_dict);
        let action = d.map(|d| {
            let kind = d.get(b"S").and_then(Object::as_name);
            let kind = kind.map(latin1).unwrap_or_default();
            let script = match d.get(b"JS") {
                Some(js) if kind == "JavaScript" => self.text(js),
                _ => None,
            };
            Action { kind, script }
        });
        if let Some(id) = a.as_reference() {
            self.actions.insert(id, action.clone());
        }
        action
    }

    /// Every page's widgets that belong to a field, by page index: what
    /// each page's `/Annots` in the page table lists. An array pages share
    /// is looked through once. A field's widgets no page lists are counted
    /// as its [`outside`](Field::outside).
    fn on_pages(&mut self, pages: &[PageFacts]) -> BTreeMap<u32, Vec<Widget>> {
        let pdf = self.pdf;
        let mut out: BTreeMap<u32, Vec<Widget>> = BTreeMap::new();
        if self.damaged.is_some() || self.widget_of.is_empty() {
            return out;
        }
        // Each array's widgets, by slot.
        let mut arrays: HashMap<*const [Object], Vec<(usize, ObjectId)>> = HashMap::new();
        let mut count = 0;
        for (index, page) in pages.iter().enumerate() {
            let widgets = arrays.entry(Arc::as_ptr(&page.annots)).or_insert_with(|| {
                page.annots
                    .iter()
                    .enumerate()
                    .filter_map(|(slot, a)| Some((slot, a.as_reference()?)))
                    .filter(|(_, id)| self.widget_of.contains_key(id))
                    .collect()
            });
            for &(slot, id) in widgets.iter() {
                let of = self.widget_of[&id];
                count += 1;
                if count > self.limits.widgets {
                    let n = self.limits.widgets;
                    self.damage(format!("it has more than {n} widgets on pages"));
                    return out;
                }
                let Some(d) = pdf.dictionary(id) else {
                    continue;
                };
                let widget = self.widget(id, d, self.fields[of.field].id, slot, of);
                if self.damaged.is_some() {
                    return out;
                }
                out.entry(index as u32).or_default().push(widget);
            }
        }
        let listed: HashSet<ObjectId> = arrays.into_values().flatten().map(|(_, id)| id).collect();
        for (id, of) in &self.widget_of {
            if !listed.contains(id) {
                let outside = &mut self.fields[of.field].outside;
                *outside = outside.saturating_add(1);
            }
        }
        out
    }

    fn widget(
        &mut self,
        id: ObjectId,
        d: &Dictionary,
        field: FieldId,
        slot: usize,
        of: WidgetOf,
    ) -> Widget {
        let pdf = self.pdf;
        let kid = of.kid;
        let get = |key: &[u8]| d.get(key).and_then(|o| resolve(pdf, o));
        let rect = get(b"Rect")
            .and_then(Object::as_array)
            .and_then(|r| <&[Object; 4]>::try_from(r).ok())
            .map(|r| {
                let v = |o: &Object| clamp_coord(number_of(pdf, o).unwrap_or(0.0) as f32);
                let (x0, y0, x1, y1) = (v(&r[0]), v(&r[1]), v(&r[2]), v(&r[3]));
                [x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)]
            })
            .unwrap_or_default();
        let mk = get(b"MK").and_then(Object::as_dict);
        let mk_get = |key: &[u8]| mk.and_then(|m| m.get(key)).and_then(|o| resolve(pdf, o));
        let rotation = mk_get(b"R").and_then(Object::as_integer).unwrap_or(0);
        // `/CA`, `/RC` and `/AC`, each a text, in that order.
        let mut caption = |key: &[u8]| match mk_get(key) {
            Some(text) => self.text(text).map(|c| c.to_string()),
            None => None,
        };
        let (normal, rollover, down) = (caption(b"CA"), caption(b"RC"), caption(b"AC"));
        let mk = Mk {
            background: mk_get(b"BG").and_then(|c| color(pdf, c)),
            border_color: mk_get(b"BC").and_then(|c| color(pdf, c)),
            rotation: match rotation.rem_euclid(360) {
                r @ (90 | 180 | 270) => r as u16,
                _ => 0,
            },
            caption: normal,
            rollover,
            down,
            text_position: mk_get(b"TP").and_then(text_position),
            icon: mk_get(b"I").is_some_and(|i| i.as_stream().is_some()),
        };
        let border = match (
            get(b"BS").and_then(Object::as_dict),
            get(b"Border").and_then(Object::as_array),
        ) {
            (Some(bs), _) => {
                let style = match bs.get(b"S").and_then(|s| resolve(pdf, s)) {
                    Some(s) => match s.as_name() {
                        Some(b"D") => BorderStyle::Dashed,
                        Some(b"B") => BorderStyle::Beveled,
                        Some(b"I") => BorderStyle::Inset,
                        Some(b"U") => BorderStyle::Underline,
                        _ => BorderStyle::Solid,
                    },
                    None => BorderStyle::Solid,
                };
                let width = (bs.get(b"W").and_then(|w| number_of(pdf, w)))
                    .map_or(1.0, |w| clamp_width(w as f32));
                let dash = array(pdf, bs.get(b"D")).map_or_else(Vec::new, |d| dashes(pdf, d));
                Some(Border {
                    width,
                    style,
                    dash: dashed(style, dash),
                })
            }
            // An array of three or more: its third element is the width, 0
            // where it isn't a number, and its fourth, an array, is a dash
            // array, which makes the border dashed. A shorter one is no
            // border record. As `pdf-annots` reads a `/Border`.
            (None, Some(b)) if b.len() >= 3 => {
                let width = number_of(pdf, &b[2]).map_or(0.0, |w| clamp_width(w as f32));
                let dash = array(pdf, b.get(3)).map(|d| dashes(pdf, d));
                let style = match dash {
                    Some(_) => BorderStyle::Dashed,
                    None => BorderStyle::Solid,
                };
                Some(Border {
                    width,
                    style,
                    dash: dashed(style, dash.unwrap_or_default()),
                })
            }
            _ => None,
        };
        let state = d.get(b"AS").and_then(Object::as_name);
        let state = state.filter(|s| self.spend(s.len())).map(latin1);
        let normal = normal_of(pdf, d);
        let mut states = Vec::new();
        let mut on_state = None;
        if let Some(PdfValue::Dict(n)) = normal.map(Object::value) {
            for (name, _) in n.iter() {
                if !self.spend(name.len()) {
                    break;
                }
                states.push(latin1(name));
            }
            // One of the names just counted, so nothing more is.
            on_state = on_state_of(pdf, n);
        }
        let has_appearance = match normal.map(Object::value) {
            Some(PdfValue::Stream(_)) => true,
            Some(PdfValue::Dict(_)) => !states.is_empty(),
            _ => false,
        };
        let (da, align) = match kid {
            true => (self.da(d), align(pdf, d)),
            false => (None, None),
        };
        // `/H`: a name, or a string's bytes, as PDFium reads one.
        let pressed = get(b"H")
            .and_then(|h| h.as_name().or_else(|| h.as_bytes()))
            .is_some_and(|h| matches!(h, b"P" | b"T"));
        let actions = self.actions(d, true);
        Widget {
            field,
            object: id,
            merged: !kid,
            slot,
            index: of.index,
            rect,
            flags: number(pdf, d, b"F").unwrap_or(0) as u32,
            da,
            mk,
            border,
            align,
            state,
            states,
            on_state,
            has_appearance,
            pressed,
            actions,
        }
    }
}

/// `/MK /TP`, `o` followed: an integer, or a finite real taken toward 0,
/// each held within `i32`, as PDFium reads a number as an integer; none
/// for anything else.
fn text_position(o: &Object) -> Option<i32> {
    match o.as_integer() {
        Some(n) => Some(n.clamp(i32::MIN.into(), i32::MAX.into()) as i32),
        // `as` takes a real toward 0 and holds it within `i32`.
        None => o.as_number().filter(|v| v.is_finite()).map(|v| v as i32),
    }
}

/// Widget `d`'s normal appearance, `/AP /N`, each followed: a stream, or
/// its states.
fn normal_of<'a, P: Objects + ?Sized>(pdf: &'a P, d: &'a Dictionary) -> Option<&'a Object> {
    (d.get(b"AP").and_then(|ap| resolve(pdf, ap)))
        .and_then(Object::as_dict)
        .and_then(|ap| ap.get(b"N"))
        .and_then(|n| resolve(pdf, n))
}

/// The name of the on state of a normal appearance's states `n`: of its
/// names that aren't empty or `Off` and whose value is a stream, the least
/// by its bytes, as the family reads one under PDFium's rules (as PDFium
/// keeps a dictionary's names in order). Only the names are sorted, and a
/// value is followed until one is a stream.
fn on_state_of<P: Objects + ?Sized>(pdf: &P, n: &Dictionary) -> Option<String> {
    let mut names: Vec<(&[u8], &Object)> = (n.iter())
        .filter(|(name, _)| !name.is_empty() && *name != b"Off")
        .collect();
    names.sort_by_key(|&(name, _)| name);
    let is_stream = |o: &Object| resolve(pdf, o).is_some_and(|o| o.as_stream().is_some());
    let (name, _) = names.into_iter().find(|&(_, o)| is_stream(o))?;
    Some(latin1(name))
}

/// A value read before the field's kind was known, made to fit it: a
/// button's is a state, a text or choice field's is text.
fn fit(v: Value, kind: FieldKind) -> Value {
    match (v, kind) {
        (Value::Text(t), FieldKind::CheckBox | FieldKind::Radio) => Value::State(t),
        (
            Value::State(s),
            FieldKind::Text | FieldKind::Combo | FieldKind::List | FieldKind::Unknown,
        ) => Value::Text(s),
        (v, _) => v,
    }
}

fn cycle() -> String {
    "a field is its own ancestor in /Kids".into()
}

/// Every field and widget the tree under `roots` lists as a kid; none when
/// the roots and the `/Kids` of every node they reach come to more than
/// `max` entries. The read walks no list this doesn't, so this bounds it
/// too.
fn listed_as_kids<P: Objects + ?Sized>(
    pdf: &P,
    roots: &[ObjectId],
    max: usize,
) -> Option<HashSet<ObjectId>> {
    let mut out = HashSet::new();
    let mut seen = HashSet::new();
    let mut todo = roots.to_vec();
    let mut entries = roots.len();
    while let Some(id) = todo.pop() {
        if entries > max {
            return None;
        }
        if !seen.insert(id) {
            continue;
        }
        let kids = pdf
            .dictionary(id)
            .and_then(|d| d.get(b"Kids"))
            .and_then(|k| resolve(pdf, k))
            .and_then(Object::as_array)
            .unwrap_or_default();
        entries += kids.len();
        for kid in kids.iter().filter_map(Object::as_reference) {
            out.insert(kid);
            todo.push(kid);
        }
    }
    (entries <= max).then_some(out)
}

fn is_widget(d: &Dictionary) -> bool {
    d.get(b"Subtype")
        .and_then(Object::as_name)
        .is_some_and(|s| s == b"Widget")
}

/// Bytes as Latin-1 text, one character each: lossless, for names.
fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

fn number<P: Objects + ?Sized>(pdf: &P, d: &Dictionary, key: &[u8]) -> Option<i64> {
    resolve(pdf, d.get(key)?)?.as_integer()
}

fn boolean<P: Objects + ?Sized>(pdf: &P, d: &Dictionary, key: &[u8]) -> bool {
    d.get(key)
        .and_then(|o| resolve(pdf, o))
        .and_then(Object::as_bool)
        .unwrap_or(false)
}

/// `/Q`, when it's 0, 1 or 2.
fn align<P: Objects + ?Sized>(pdf: &P, d: &Dictionary) -> Option<u8> {
    number(pdf, d, b"Q")
        .filter(|q| (0..=2).contains(q))
        .map(|q| q as u8)
}

fn clamp_coord(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(-MAX_COORD, MAX_COORD)
    } else {
        0.0
    }
}

fn clamp_width(w: f32) -> f32 {
    if w.is_finite() {
        w.clamp(0.0, 100.0)
    } else {
        1.0
    }
}

/// The number `o` is, followed where it's a reference.
fn number_of<P: Objects + ?Sized>(pdf: &P, o: &Object) -> Option<f64> {
    resolve(pdf, o)?.as_number()
}

/// The array `o` is, followed where it's a reference.
fn array<'a, P: Objects + ?Sized>(pdf: &'a P, o: Option<&'a Object>) -> Option<&'a [Object]> {
    resolve(pdf, o?)?.as_array()
}

/// The most elements of a border's dash array that are read: `pdf-annots`'
/// count for an annotation's border, which names it nowhere public; the
/// fields dump sets the two readings beside each other.
pub const MAX_DASHES: usize = 32;

/// The one dash of a dashed border that gives no dash array: `[3]`, ISO
/// 32000-2's default (12.5.4, Table 168), as `pdf-annots` fills it in.
pub const DEFAULT_DASH: f32 = 3.0;

/// A border's dash array: the first [`MAX_DASHES`] of its elements, each
/// followed where it's a reference, the numbers among them that are
/// finite and not negative, as `pdf-annots` reads one.
fn dashes<P: Objects + ?Sized>(pdf: &P, d: &[Object]) -> Vec<f32> {
    (d.iter().take(MAX_DASHES))
        .filter_map(|o| number_of(pdf, o))
        .map(|v| v as f32)
        .filter(|v| v.is_finite() && *v >= 0.0)
        .collect()
}

/// `dash`, or the default ([`DEFAULT_DASH`]) for a dashed border that
/// gives none.
fn dashed(style: BorderStyle, dash: Vec<f32>) -> Vec<f32> {
    match style == BorderStyle::Dashed && dash.is_empty() {
        true => vec![DEFAULT_DASH],
        false => dash,
    }
}

/// An `/MK` color, each part followed where it's a reference: none for an
/// empty array (transparent) or one of the wrong length.
fn color<P: Objects + ?Sized>(pdf: &P, o: &Object) -> Option<Color> {
    let parts: Vec<f32> = o
        .as_array()?
        .iter()
        .map(|p| match number_of(pdf, p) {
            Some(v) if (v as f32).is_finite() => (v as f32).clamp(0.0, 1.0),
            _ => 0.0,
        })
        .collect();
    match parts[..] {
        [g] => Some(Color::Gray(g)),
        [r, g, b] => Some(Color::Rgb([r, g, b])),
        [c, m, y, k] => Some(Color::Cmyk([c, m, y, k])),
        _ => None,
    }
}

/// Document-level scripts in the catalog's `/Names /JavaScript`: those
/// that would run, and Adobe's version checks (`!ADBE::`). Each node and
/// array is read once, however many nodes list it, and the walk stops
/// after [`FormLimits::scripts`] kids and scripts.
fn document_scripts(pdf: &LazyPdf, limits: &FormLimits) -> (usize, usize) {
    let tree = pdf
        .catalog()
        .ok()
        .and_then(|c| c.get(b"Names"))
        .and_then(|n| resolve(pdf, n))
        .and_then(Object::as_dict)
        .and_then(|n| n.get(b"JavaScript"));
    let mut todo: Vec<&Object> = tree.into_iter().collect();
    let mut seen = HashSet::new();
    let (mut entries, mut scripts, mut checks) = (0, 0, 0);
    'walk: while let Some(node) = todo.pop() {
        if entries > limits.scripts {
            break;
        }
        let Some(d) = once(pdf, &mut seen, node).and_then(Object::as_dict) else {
            continue;
        };
        let mut array = |key: &[u8]| once(pdf, &mut seen, d.get(key)?)?.as_array();
        let kids = array(b"Kids").unwrap_or_default();
        let names = array(b"Names").unwrap_or_default();
        entries += kids.len();
        todo.extend(kids);
        // Pairs of a name and an action.
        for name in names.iter().step_by(2) {
            entries += 1;
            if entries > limits.scripts {
                break 'walk;
            }
            let name = resolve(pdf, name).and_then(Object::as_bytes);
            if name.is_some_and(|n| n.starts_with(b"!ADBE::")) {
                checks += 1;
            } else {
                scripts += 1;
            }
        }
    }
    (scripts, checks)
}

#[cfg(test)]
mod tests;

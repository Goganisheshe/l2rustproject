//! Reader for `system/interface.xdat`, the client's UI layout: every window
//! with its controls (textures, buttons, bars, item grids...), anchors and
//! sizes. The layout follows acmi's xdat_editor schema for Salvation
//! (`etoa5`): hand-written readers for the classes with custom IO and the
//! generated field lists of `xdat_schema.rs` for the rest.

use crate::reader::Reader;
use crate::Error;

/// How a @DefaultIO field is stored.
#[derive(Debug, Clone, Copy)]
pub enum Kind {
    Int,
    Bool,
    Float,
    Str,
    /// ARGB.
    Color,
    /// RGBA.
    Rgba,
    EnumInt,
    EnumStr,
    List(&'static str),
    Entity(&'static str),
}

#[derive(Debug, Clone)]
pub enum Value {
    Int(i32),
    Float(f32),
    Str(String),
    /// Packed color as stored (ARGB or RGBA, see the field kind).
    Color(u32),
    List(Vec<Element>),
    Entity(Box<Element>),
}

/// One UI object: a window or control, or a nested record.
#[derive(Debug, Clone, Default)]
pub struct Element {
    pub class: String,
    pub fields: Vec<(String, Value)>,
}

impl Element {
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.fields.iter().find(|f| f.0 == name).map(|f| &f.1)
    }
    pub fn str(&self, name: &str) -> &str {
        match self.get(name) {
            Some(Value::Str(s)) => s,
            _ => "",
        }
    }
    pub fn int(&self, name: &str) -> i32 {
        match self.get(name) {
            Some(Value::Int(i)) => *i,
            Some(Value::Float(f)) => *f as i32,
            _ => 0,
        }
    }
    pub fn float(&self, name: &str) -> f32 {
        match self.get(name) {
            Some(Value::Float(f)) => *f,
            Some(Value::Int(i)) => *i as f32,
            _ => 0.0,
        }
    }
    pub fn flag(&self, name: &str) -> bool {
        self.int(name) == 1
    }
    pub fn color(&self, name: &str) -> u32 {
        match self.get(name) {
            Some(Value::Color(c)) => *c,
            _ => 0,
        }
    }
    pub fn list(&self, name: &str) -> &[Element] {
        match self.get(name) {
            Some(Value::List(l)) => l,
            _ => &[],
        }
    }
    /// Child controls of a window.
    pub fn children(&self) -> &[Element] {
        self.list("children")
    }
    pub fn name(&self) -> &str {
        self.str("name")
    }
}

#[derive(Debug, Clone, Default)]
pub struct Xdat {
    pub shortcuts: Vec<Element>,
    pub windows: Vec<Element>,
    pub wnd_def_pos: Vec<Element>,
    pub fonts: Vec<Element>,
    pub styles: Vec<Element>,
    /// Bytes read / total, to spot schema drift.
    pub consumed: (usize, usize),
}

impl Xdat {
    pub fn window(&self, name: &str) -> Option<&Element> {
        self.windows.iter().find(|w| w.name().eq_ignore_ascii_case(name))
    }
}

struct In<'a> {
    r: Reader<'a>,
}

impl In<'_> {
    fn int(&mut self) -> Result<i32, Error> {
        Ok(self.r.u32()? as i32)
    }
    fn float(&mut self) -> Result<f32, Error> {
        self.r.f32()
    }
    fn string(&mut self) -> Result<String, Error> {
        let len = self.r.compact()?;
        if len == 0 {
            return Ok(String::new());
        }
        if len > 0 {
            let b = self.r.bytes(len as usize)?;
            Ok(b[..b.len() - 1].iter().map(|&c| c as char).collect())
        } else {
            let b = self.r.bytes((-2 * len) as usize)?;
            let w: Vec<u16> = b.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            Ok(String::from_utf16_lossy(&w[..w.len().saturating_sub(1)]))
        }
    }

    fn field(&mut self, e: &mut Element, name: &str, kind: Kind) -> Result<(), Error> {
        let v = match kind {
            Kind::Int | Kind::Bool | Kind::EnumInt => Value::Int(self.int()?),
            Kind::Float => Value::Float(self.float()?),
            Kind::Str | Kind::EnumStr => Value::Str(self.string()?),
            Kind::Color | Kind::Rgba => Value::Color(self.int()? as u32),
            Kind::List(t) => {
                let n = if name == "additionalState" { self.r.compact()? } else { self.int()? };
                if !(0..=100_000).contains(&n) {
                    return Err(Error::Format(format!("xdat: bad list length {n} for {name}")));
                }
                let mut v = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    v.push(if t == "DefaultProperty" { self.ui_entity()? } else { self.entity(t)? });
                }
                Value::List(v)
            }
            Kind::Entity(t) => Value::Entity(Box::new(self.entity(t)?)),
        };
        e.fields.push((name.to_string(), v));
        Ok(())
    }

    /// A control preceded by its class name.
    fn ui_entity(&mut self) -> Result<Element, Error> {
        let class = self.string()?;
        self.entity(&class)
    }

    fn entity(&mut self, class: &str) -> Result<Element, Error> {
        let mut e = Element { class: class.to_string(), fields: Vec::new() };
        match class {
            "Window" => {
                self.default_property(&mut e)?;
                self.window(&mut e)?;
            }
            "FontData" => self.font_data(&mut e)?,
            "Element" => self.head_display_element(&mut e)?,
            "ShortcutSpecialItem" => {
                self.field(&mut e, "key", Kind::EnumInt)?;
                self.field(&mut e, "command", Kind::Str)?;
            }
            _ => {
                let Some((_, sup, fields)) = crate::xdat_schema::CLASSES.iter().find(|c| c.0 == class) else {
                    return Err(Error::Format(format!("xdat: unknown class {class}")));
                };
                if *sup == Some("DefaultProperty") {
                    self.default_property(&mut e)?;
                }
                for (name, kind) in fields.iter() {
                    self.field(&mut e, name, *kind)?;
                }
            }
        }
        Ok(e)
    }

    fn default_property(&mut self, e: &mut Element) -> Result<(), Error> {
        use Kind::*;
        for (n, k) in [
            ("name", Str),
            ("extendsName", Str),
            ("alwaysOnTop", Bool),
            ("alwaysOnBack", Bool),
            ("equalToParentSize", Bool),
            ("parentName", Str),
            ("preOrder", Str),
            ("postOrder", Str),
            ("anchored", Bool),
            ("relativeDrag", Bool),
            ("superChild", Bool),
            ("size", Bool),
        ] {
            self.field(e, n, k)?;
        }
        if e.flag("size") {
            self.field(e, "size_absolute_values", Bool)?;
            if !e.flag("size_absolute_values") {
                self.field(e, "size_percent_window", Str)?;
                self.field(e, "size_percent_width", Float)?;
                self.field(e, "size_percent_height", Float)?;
            }
            self.field(e, "width", Int)?;
            self.field(e, "height", Int)?;
        }
        self.field(e, "usePosition", Bool)?;
        if e.flag("usePosition") {
            self.field(e, "relativePoint", EnumInt)?;
            self.field(e, "anchorPoint", EnumInt)?;
            self.field(e, "relativeTo", Str)?;
            self.field(e, "x", Float)?;
            self.field(e, "y", Float)?;
        }
        self.field(e, "useFont", Bool)?;
        if e.flag("useFont") {
            self.field(e, "styleName", Str)?;
            self.field(e, "fontName", Str)?;
            self.field(e, "fontColor", Color)?;
            self.field(e, "fontLine", EnumInt)?;
        }
        for (n, k) in [
            ("virtual", Bool),
            ("unk24", Int),
            ("tooltipType", Str),
            ("tooltipText", Int),
            ("tooltipAppearanceTime", Int),
            ("scrollBarOffsetX", Int),
            ("scrollBarOffsetY", Int),
            ("scrollBarOffsetHeight", Int),
        ] {
            self.field(e, n, k)?;
        }
        Ok(())
    }

    fn window(&mut self, e: &mut Element) -> Result<(), Error> {
        use Kind::*;
        for (n, k) in [
            ("parent", Str),
            ("backTex", Str),
            ("script", Str),
            ("state", Str),
            ("frame", Bool),
            ("iconable", Bool),
            ("stuckable", Bool),
            ("hidden", Bool),
            ("alwaysFullAlpha", Bool),
            ("savePosition", Bool),
            ("saveSize", Bool),
            ("title", Int),
            ("resizeFrame", Bool),
            ("frameSize", EnumInt),
            ("frameDirection", EnumInt),
            ("exitbutton", Bool),
            ("movable", Bool),
            ("draggable", Bool),
            ("resizeFrameDirection", EnumInt),
            ("resizeFrameX", Float),
            ("resizeFrameY", Float),
            ("resizeFrameWidth", Float),
            ("resizeFrameHeight", Float),
            ("resizeMaxIncrease", Int),
            ("drawerDirection", EnumInt),
        ] {
            self.field(e, n, k)?;
        }
        if e.int("drawerDirection") > 0 {
            self.field(e, "offsetX", Int)?;
            self.field(e, "offsetY", Int)?;
            self.field(e, "directionFixed", Bool)?;
        } else {
            self.r.skip(12)?;
        }
        self.field(e, "ownerWindow", Str)?;
        self.field(e, "showAnimType", EnumInt)?;
        self.field(e, "hideAnimType", EnumInt)?;
        for anim in ["show", "hide"] {
            if e.int(&format!("{anim}AnimType")) > 0 {
                self.field(e, &format!("{anim}AnimDirection"), EnumInt)?;
                self.field(e, &format!("{anim}AnimSeconds"), Float)?;
            } else {
                self.r.skip(8)?;
            }
        }
        for (n, k) in [
            ("iconName", Str),
            ("tooltipIdx", Int),
            ("hookKeyInput", Bool),
            ("workingConfiguration", Str),
            ("leftTextureName", Str),
            ("midTextureName", Str),
            ("rightTextureName", Str),
            ("minimizeBtnTextureNormal", Str),
            ("minimizeBtnTexturePushed", Str),
            ("closeBtnTextureNormal", Str),
            ("closeBtnTexturePushed", Str),
            ("leftBackTextureWidth", Int),
            ("midBackTextureWidth", Int),
            ("additionalState", List("State")),
            ("useParentClipRect", Bool),
            ("showInArena", Bool),
            ("children", List("DefaultProperty")),
        ] {
            self.field(e, n, k)?;
        }
        Ok(())
    }

    fn font_data(&mut self, e: &mut Element) -> Result<(), Error> {
        use Kind::*;
        for (n, k) in [("name", Str), ("file", Str), ("location", Str), ("size", Int), ("index", Int), ("indexOn", Int), ("shadow", Bool)] {
            self.field(e, n, k)?;
        }
        self.field(e, "shadow_x", Int)?;
        self.field(e, "shadow_y", Int)?;
        self.field(e, "stroke", Bool)?;
        self.field(e, "stroke_large", Int)?;
        self.field(e, "lineGap", Int)?;
        self.field(e, "underlineOffset", Int)?;
        Ok(())
    }

    fn head_display_element(&mut self, e: &mut Element) -> Result<(), Error> {
        use Kind::*;
        self.field(e, "reserved", Int)?;
        self.field(e, "drawType", EnumInt)?;
        if e.int("drawType") == 1 {
            self.field(e, "texName", Str)?;
            for n in ["u1", "v1", "u2", "v2"] {
                self.field(e, n, Int)?;
            }
        } else {
            self.field(e, "systemMsg", Int)?;
            self.field(e, "color", Color)?;
        }
        Ok(())
    }

    fn list(&mut self, class: &str) -> Result<Vec<Element>, Error> {
        let n = self.int()?;
        (0..n).map(|_| self.entity(class)).collect()
    }
}

/// Parses `interface.xdat`. Windows that fail to parse stop the read; the
/// ones before them are kept (`Xdat::windows`).
pub fn parse(data: &[u8]) -> Result<Xdat, Error> {
    let mut i = In { r: Reader::new(data) };
    let mut x = Xdat { shortcuts: i.list("Shortcut")?, ..Default::default() };
    let n = i.int()?;
    for _ in 0..n {
        let mut w = Element { class: "Window".into(), fields: Vec::new() };
        let start = i.r.pos;
        i.default_property(&mut w)?;
        i.window(&mut w).map_err(|e| Error::Format(format!("window {} at {start}: {e}", w.name())))?;
        x.windows.push(w);
    }
    i.int()?;
    x.wnd_def_pos = i.list("WndDefPos")?;
    x.fonts = i.list("Font")?;
    x.styles = i.list("Style")?;
    x.consumed = (i.r.pos, data.len());
    Ok(x)
}

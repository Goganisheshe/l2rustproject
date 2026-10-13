use std::path::Path;

use crate::{crypt, Error, Reader};

/// UE2 object reference: 0 = null, >0 = export index + 1, <0 = -(import index + 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ObjectRef {
    #[default]
    Null,
    Export(usize),
    Import(usize),
}

impl ObjectRef {
    pub fn from_raw(v: i32) -> Self {
        match v {
            0 => ObjectRef::Null,
            v if v > 0 => ObjectRef::Export(v as usize - 1),
            v => ObjectRef::Import((-v) as usize - 1),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Import {
    pub class_package: usize,
    pub class_name: usize,
    pub package: ObjectRef,
    pub object_name: usize,
}

#[derive(Debug, Clone)]
pub struct Export {
    pub class: ObjectRef,
    pub super_: ObjectRef,
    pub package: ObjectRef,
    pub object_name: usize,
    pub flags: u32,
    pub serial_size: usize,
    pub serial_offset: usize,
}

#[derive(Debug)]
pub struct Package {
    pub name: String,
    pub version: u16,
    pub licensee: u16,
    pub flags: u32,
    pub names: Vec<String>,
    pub imports: Vec<Import>,
    pub exports: Vec<Export>,
    pub data: Vec<u8>,
}

impl Package {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref();
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self::from_bytes(name, std::fs::read(path)?)
    }

    pub fn from_bytes(name: String, raw: Vec<u8>) -> Result<Self, Error> {
        let data = crypt::decrypt(raw)?;
        let mut r = Reader::new(&data);
        let magic = r.u32()?;
        if magic != crypt::PACKAGE_MAGIC {
            return Err(Error::BadMagic(magic));
        }
        let version = r.u16()?;
        let licensee = r.u16()?;
        let flags = r.u32()?;
        let name_count = r.i32()? as usize;
        let name_offset = r.i32()? as usize;
        let export_count = r.i32()? as usize;
        let export_offset = r.i32()? as usize;
        let import_count = r.i32()? as usize;
        let import_offset = r.i32()? as usize;

        let mut r = Reader::at(&data, name_offset);
        let mut names = Vec::with_capacity(name_count);
        for _ in 0..name_count {
            names.push(r.fstring()?);
            r.u32()?; // object flags
        }

        let mut r = Reader::at(&data, import_offset);
        let mut imports = Vec::with_capacity(import_count);
        for _ in 0..import_count {
            imports.push(Import {
                class_package: r.compact()? as usize,
                class_name: r.compact()? as usize,
                package: ObjectRef::from_raw(r.i32()?),
                object_name: r.compact()? as usize,
            });
        }

        let mut r = Reader::at(&data, export_offset);
        let mut exports = Vec::with_capacity(export_count);
        for _ in 0..export_count {
            let class = ObjectRef::from_raw(r.compact()?);
            let super_ = ObjectRef::from_raw(r.compact()?);
            let package = ObjectRef::from_raw(r.i32()?);
            let object_name = r.compact()? as usize;
            let flags = r.u32()?;
            let serial_size = r.compact()? as usize;
            let serial_offset = if serial_size > 0 { r.compact()? as usize } else { 0 };
            exports.push(Export { class, super_, package, object_name, flags, serial_size, serial_offset });
        }

        Ok(Self { name, version, licensee, flags, names, imports, exports, data })
    }

    pub fn name(&self, idx: usize) -> &str {
        self.names.get(idx).map(String::as_str).unwrap_or("<bad-name>")
    }

    pub fn object_name(&self, r: ObjectRef) -> &str {
        match r {
            ObjectRef::Null => "None",
            ObjectRef::Export(i) => self.name(self.exports[i].object_name),
            ObjectRef::Import(i) => self.name(self.imports[i].object_name),
        }
    }

    /// Class name of an export ("Class" for UClass exports which have class = null).
    pub fn class_name(&self, export: &Export) -> &str {
        match export.class {
            ObjectRef::Null => "Class",
            r => self.object_name(r),
        }
    }

    /// Dotted path like `Group.Object` (package name omitted).
    pub fn full_name(&self, r: ObjectRef) -> String {
        let (name, outer) = match r {
            ObjectRef::Null => return "None".into(),
            ObjectRef::Export(i) => (self.object_name(r), self.exports[i].package),
            ObjectRef::Import(i) => (self.object_name(r), self.imports[i].package),
        };
        match outer {
            ObjectRef::Null => name.to_string(),
            o => format!("{}.{}", self.full_name(o), name),
        }
    }

    pub fn export_data(&self, idx: usize) -> &[u8] {
        let e = &self.exports[idx];
        &self.data[e.serial_offset..e.serial_offset + e.serial_size]
    }

    pub fn exports_of_class<'a>(&'a self, class: &'a str) -> impl Iterator<Item = usize> + 'a {
        (0..self.exports.len()).filter(move |&i| self.class_name(&self.exports[i]).eq_ignore_ascii_case(class))
    }

    /// Finds an export by object name or dotted `Group.Object` path.
    pub fn find_export(&self, name: &str) -> Option<usize> {
        (0..self.exports.len()).find(|&i| {
            let r = ObjectRef::Export(i);
            self.object_name(r).eq_ignore_ascii_case(name) || self.full_name(r).eq_ignore_ascii_case(name)
        })
    }

    pub fn find_name(&self, s: &str) -> Option<usize> {
        self.names.iter().position(|n| n.eq_ignore_ascii_case(s))
    }
}

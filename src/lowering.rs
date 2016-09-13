// Copyright 2016 The RLS Project Developers.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

// For processing the raw save-analysis data from rustc into rustw's in-memory representation.

use super::raw::{self, Format};
use super::{Analysis, Span, NULL, Def};

use std::collections::HashMap;

pub fn lower(raw_analysis: Vec<raw::Analysis>, project_dir: &str) -> Analysis {
    let mut result = Analysis::new(project_dir);
    let mut master_crate_map = HashMap::new();
    for krate in raw_analysis.into_iter() {
        CrateReader::read_crate(&mut result, &mut master_crate_map, krate, project_dir);
    }

    result
}

pub fn lower_span(raw_span: &raw::SpanData, project_dir: Option<&str>) -> Span {
    let file_name = &raw_span.file_name;
    let file_name = if file_name.starts_with('/') {
        file_name.clone()
    } else {
        format!("{}/{}", project_dir.expect("Required project directory, but not supplied"), file_name)
    };
    Span {
        file_name: file_name,
        line_start: raw_span.line_start,
        column_start: raw_span.column_start,
        line_end: raw_span.line_end,
        column_end: raw_span.column_end,
    }
}

struct CrateReader {
    crate_map: Vec<u8>,
}

impl CrateReader {
    fn from_prelude(mut prelude: raw::CratePreludeData, master_crate_map: &mut HashMap<String, u8>) -> CrateReader {
        // println!("building crate map for {}", prelude.crate_name);
        let next = master_crate_map.len() as u8;
        let mut crate_map = vec![*master_crate_map.entry(prelude.crate_name.clone()).or_insert_with(|| next)];
        // println!("  {} -> {}", prelude.crate_name, master_crate_map[&prelude.crate_name]);

        prelude.external_crates.sort_by(|a, b| a.num.cmp(&b.num));
        for c in prelude.external_crates {
            assert!(c.num == crate_map.len() as u32);
            let next = master_crate_map.len() as u8;
            crate_map.push(*master_crate_map.entry(c.name.clone()).or_insert_with(|| next));
            // println!("  {} -> {}", c.name, master_crate_map[&c.name]);
        }

        CrateReader {
            crate_map: crate_map,
        }
    }

    fn read_crate(analysis: &mut Analysis,
                  master_crate_map: &mut HashMap<String, u8>,
                  krate: raw::Analysis,
                  project_dir: &str) {
        let crate_name = krate.prelude.as_ref().unwrap().crate_name.clone();
        let reader = CrateReader::from_prelude(krate.prelude.unwrap(), master_crate_map);

        for i in krate.imports {
            let span = lower_span(&i.span, Some(project_dir));
            let id = reader.id_from_compiler_id(&i.id);
            analysis.def_id_for_span.insert(span, id);

            let def = Def {
                kind: raw::DefKind::Import,
                span: i.span,
                name: i.name,
                value: i.value,
                qualname: String::new(),
                parent: None,
                docs: String::new(),
            };
            analysis.defs.insert(id, def);
        }
        for mut d in krate.defs {
            let span = lower_span(&d.span, Some(project_dir));
            let id = reader.id_from_compiler_id(&d.id);
            if id != NULL && !analysis.defs.contains_key(&id) {
                if krate.kind == Format::Json {
                    let file_name = span.file_name.clone();
                    analysis.defs_per_file.entry(file_name).or_insert_with(|| vec![]).push(id);

                    analysis.def_id_for_span.insert(span, id);
                    analysis.def_names.entry(d.name.clone()).or_insert_with(|| vec![]).push(id);
                } else {
                    // TODO gross hack - take me out, and do something better in rustc
                    // TODO shit, I can't even remember why we do this - it makes no sense :-s
                    if d.kind == super::raw::DefKind::Struct {
                        d.value = String::new();
                    }
                }
                let def = Def {
                    kind: d.kind,
                    span: d.span,
                    name: d.name,
                    value: d.value,
                    qualname: format!("{}{}", crate_name, d.qualname),
                    parent: d.parent.map(|id| reader.id_from_compiler_id(&id)),
                    docs: if let Some(index) = d.docs.find("\n\n") {
                        d.docs[..index].to_owned()
                    } else {
                        d.docs
                    },
                };

                analysis.defs.insert(id, def);
            }
        }
        for r in krate.refs {
            let def_id = reader.id_from_compiler_id(&r.ref_id);
            let span = lower_span(&r.span, Some(project_dir));
            if def_id != NULL && analysis.defs.contains_key(&def_id) && !analysis.def_id_for_span.contains_key(&span) {

                //println!("record ref {:?} {:?} {:?} {}", r.kind, span, r.ref_id, id);
                analysis.def_id_for_span.insert(span.clone(), def_id);
                analysis.ref_spans.entry(def_id).or_insert_with(|| vec![]).push(span);
            }
        }
    }

    // TODO need to handle std libraries too.
    fn id_from_compiler_id(&self, id: &raw::CompilerId) -> u32 {
        if id.krate == NULL || id.index == NULL {
            return NULL;
        }
        // We build an id by looking up the local crate number into a global crate number and using
        // that for the 8 high order bits, and use the least significant 24 bits of the index part
        // of the def index as the low order bits.
        let krate = self.crate_map[id.krate as usize] as u32;
        let crate_local = id.index & 0x00ffffff;
        krate << 24 | crate_local
    }
}

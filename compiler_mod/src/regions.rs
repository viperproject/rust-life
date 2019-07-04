// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

/// Code for finding `rustc::ty::sty::RegionVid` associated with local
/// reference typed variables.

use facts;
use dump_borrowck_info::regex::Regex;
use dump_borrowck_info::rustc::mir;
use dump_borrowck_info::rustc_data_structures::indexed_vec::Idx;
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead};
use std::path::Path;
use std::thread::LocalKey;

pub fn load_variable_regions(path: &Path) -> io::Result<HashMap<mir::Local, facts::Region>> {
    trace!("[enter] load_variable_regions(path={:?})", path);
    let mut variable_regions = HashMap::new();
    let file = File::open(path)?;
    // This (all remaining code of this function) is not fully correct, it seems to not read all regions
    // that are associated with some locals. E.g "local 4 rvid 16" is not emitted for find_error_path_ex0.rs,
    // but it seems to be in the mir quite clearly and it would probably improve the final result if it would be
    // returned by this function.
    let fn_sig = Regex::new(r"^fn [a-zA-Z\d_]+\((?P<args>.*)\) -> (?P<result>.*)\{$").unwrap();
    let arg = Regex::new(r"^_(?P<local>\d+): &'_#(?P<rvid>\d+)r (mut)? [a-zA-Z\d_]+\s*$").unwrap();
    let local = Regex::new(r"^\s+(let )?(mut )?_(?P<local>\d+): &'_#(?P<rvid>\d+)r ").unwrap();
    let local2 = Regex::new(r"^\s+(let )?(mut )?_(?P<local>\d+): ([a-zA-Z]+::[a-zA-Z]+::[a-zA-Z]+<\[?)?&'_#(?P<rvid>\d+)r ").unwrap();
    let local3 = Regex::new(r"^\s+(let )?(mut )?_(?P<local>\d+): &'_#(\d+)r (mut )?([a-zA-Z]+::[a-zA-Z]+::[a-zA-Z]+<\[?)?&'_#(?P<rvid>\d+)r ").unwrap();
    for line in io::BufReader::new(file).lines() {
        let line = line?;
        if let Some(caps) = fn_sig.captures(&line) {
            debug!("args: {} result: {}", &caps["args"], &caps["result"]);
            for arg_str in (&caps["args"]).split(", ") {
                if let Some(arg_caps) = arg.captures(arg_str) {
                    debug!("arg {} rvid {}", &arg_caps["local"], &arg_caps["rvid"]);
                    let local: usize = (&arg_caps["local"]).parse().unwrap();
                    let rvid: usize = (&arg_caps["rvid"]).parse().unwrap();
                    variable_regions.insert(mir::Local::new(local), rvid.into());
                }
            }
        }
        if let Some(local_caps) = local.captures(&line) {
            debug!("local {} rvid {}", &local_caps["local"], &local_caps["rvid"]);
            let local: usize = (&local_caps["local"]).parse().unwrap();
            let rvid: usize = (&local_caps["rvid"]).parse().unwrap();
            variable_regions.insert(mir::Local::new(local), rvid.into());
        }
        if let Some(local2_caps) = local2.captures(&line) {
            debug!("local {} rvid {}", &local2_caps["local"], &local2_caps["rvid"]);
            let local: usize = (&local2_caps["local"]).parse().unwrap();
            let rvid: usize = (&local2_caps["rvid"]).parse().unwrap();
            variable_regions.insert(mir::Local::new(local), rvid.into());
        }
        if let Some(local3_caps) = local3.captures(&line) {
            debug!("local {} rvid {}", &local3_caps["local"], &local3_caps["rvid"]);
            let local: usize = (&local3_caps["local"]).parse().unwrap();
            let rvid: usize = (&local3_caps["rvid"]).parse().unwrap();
            variable_regions.insert(mir::Local::new(local), rvid.into());
        }
    }
    trace!("[exit] load_variable_regions");
    Ok(variable_regions)
}

pub fn load_region_to_local_map(path: &Path) -> io::Result<HashMap<facts::Region, mir::Local>> {
    let mut result: HashMap<facts::Region, mir::Local> = HashMap::new();

    let variable_definition = Regex::new(r"^\s+let (mut )?_(?P<local>\d+): (?P<type>.+)$").unwrap();
    let region_name = Regex::new(r"'_#(\d+)r").unwrap();

    let file = File::open(path)?;
    for line in io::BufReader::new(file).lines() {
        let line = line?;
        if let Some(variable_definition_caps) = variable_definition.captures(&line) {
            let local: usize = (&variable_definition_caps["local"]).parse().unwrap();
            let type_str = (&variable_definition_caps["type"]);
            for cap_reg in region_name.captures_iter(type_str) {
                let region: usize = (&cap_reg[1]).parse().unwrap();
                result.insert(region.into(), mir::Local::new(local));
            }

        }
    }
    Ok(result)
}

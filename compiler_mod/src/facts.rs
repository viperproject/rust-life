// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

/// Code for loading an manipulating Polonius facts.
///
/// This code was adapted from the
/// [Polonius](https://github.com/rust-lang-nursery/polonius/blob/master/src/facts.rs)
/// source code.

use dump_borrowck_info::csv::ReaderBuilder;
use dump_borrowck_info::regex::Regex;
use rustc::mir;
use dump_borrowck_info::rustc_data_structures::indexed_vec::Idx;
use dump_borrowck_info::serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::hash::Hash;
use std::path::Path;
use std::str::FromStr;
use std::fmt;

use dump_borrowck_info::polonius_engine;


/// Macro for declaring index types for referencing interned facts.
macro_rules! index_type {
    ($typ:ident, $debug_str:ident) => {
        #[derive(Ord, PartialOrd, Eq, PartialEq, Clone, Copy, Hash)]
        pub struct $typ(usize);

        impl From<usize> for $typ {
            fn from(index: usize) -> $typ {
                $typ {
                    0: index,
                }
            }
        }

        impl Into<usize> for $typ {
            fn into(self) -> usize {
                self.0 as usize
            }
        }

        impl polonius_engine::Atom for $typ {
            fn index(self) -> usize {
                self.into()
            }
        }

        impl fmt::Debug for $typ {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "{}{}", stringify!($debug_str), self.0)
            }
        }
    };
}

index_type!(PointIndex, P);
/// A unique identifier of a loan.
index_type!(Loan, L);
/// A unique identifier of a region.
index_type!(Region, R);

impl FromStr for Region {

    type Err = ();

    fn from_str(region: &str) -> Result<Self, Self::Err> {
        let re = Regex::new(r"^\\'_#(?P<id>\d+)r$").unwrap();
        let caps = re.captures(region).unwrap();
        let id: usize = caps["id"].parse().unwrap();
        Ok(Self {
            0: id,
        })
    }
}

impl FromStr for Loan {

    type Err = ();

    fn from_str(loan: &str) -> Result<Self, Self::Err> {
        let re = Regex::new(r"^bw(?P<id>\d+)$").unwrap();
        let caps = re.captures(loan).unwrap();
        let id: usize = caps["id"].parse().unwrap();
        Ok(Self {
            0: id,
        })
    }

}

/// The type of the point. Either the start of a statement or in the
/// middle of it.
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub enum PointType {
    Start,
    Mid,
}

#[derive(Debug)]
pub struct UnknownPointTypeError(String);

impl FromStr for PointType {

    type Err = UnknownPointTypeError;

    fn from_str(point_type: &str) -> Result<Self, Self::Err> {
        match point_type {
            "Start" => Ok(PointType::Start),
            "Mid" => Ok(PointType::Mid),
            _ => Err(UnknownPointTypeError(String::from(point_type))),
        }
    }
}

/// A program point used in the borrow checker analysis.
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub struct Point {
    pub location: mir::Location,
    pub typ: PointType,
}

impl FromStr for Point {

    type Err = ();

    fn from_str(point: &str) -> Result<Self, Self::Err> {
        let re = Regex::new(r"^(?P<type>Mid|Start)\(bb(?P<bb>\d+)\[(?P<stmt>\d+)\]\)$").unwrap();
        let caps = re.captures(point).unwrap();
        let point_type: PointType = caps["type"].parse().unwrap();
        let basic_block: usize = caps["bb"].parse().unwrap();
        let statement_index: usize = caps["stmt"].parse().unwrap();
        Ok(Self {
            location: mir::Location {
                block: mir::BasicBlock::new(basic_block),
                statement_index: statement_index,
            },
            typ: point_type,
        })
    }

}

pub type AllInputFacts = polonius_engine::AllFacts<Region, Loan, PointIndex>;
pub type AllOutputFacts = polonius_engine::Output<Region, Loan, PointIndex>;


/// A table that stores a mapping between interned elements of type
/// `SourceType` and their indices.
pub struct InternerTable<SourceType: Eq, IndexType: From<usize> + Copy> {
    /// For looking up from index type to source type.
    interned_elements: Vec<SourceType>,
    /// For looking up from source type into index type.
    index_elements: HashMap<SourceType, IndexType>,
}

impl<SourceType, IndexType> InternerTable<SourceType, IndexType>
    where
        SourceType: Eq + Hash + Clone,
        IndexType: Into<usize> + From<usize> + Copy,
{

    fn new() -> Self {
        Self {
            interned_elements: Vec::new(),
            index_elements: HashMap::new(),
        }
    }
    fn get_or_create_index(&mut self, element: SourceType) -> IndexType {
        if let Some(&interned) = self.index_elements.get(&element) {
            return interned;
        }

        let index = IndexType::from(self.index_elements.len());
        self.interned_elements.push(element.clone());
        *self.index_elements.entry(element).or_insert(index)
    }
    fn get_index(&self, element: &SourceType) -> IndexType {
        self.index_elements[element]
    }

    fn get_element(&self, index: IndexType) -> &SourceType {
        let index: usize = index.into();
        &self.interned_elements[index]
    }

}

trait InternTo<FromType, ToType> {

    fn intern(&mut self, element: FromType) -> ToType;

}

pub struct Interner {
    points: InternerTable<Point, PointIndex>,
}

impl Interner {

    pub fn get_point_index(&self, point: &Point) -> PointIndex {
        self.points.get_index(point)
    }


    pub fn get_point(&self, index: PointIndex) -> &Point {
        self.points.get_element(index)
    }


}

impl InternTo<String, Region> for Interner {
    fn intern(&mut self, element: String) -> Region {
        element.parse().unwrap()
    }
}

impl InternTo<String, Loan> for Interner {
    fn intern(&mut self, element: String) -> Loan {
        element.parse().unwrap()
    }
}

impl InternTo<String, PointIndex> for Interner {
    fn intern(&mut self, element: String) -> PointIndex {
        let point = element.parse().unwrap();
        self.points.get_or_create_index(point)
    }
}

impl<A, B> InternTo<(String, String), (A, B)> for Interner
    where
        Interner: InternTo<String, A>,
        Interner: InternTo<String, B>,
{
    fn intern(&mut self, (e1, e2): (String, String)) -> (A, B) {
        (self.intern(e1), self.intern(e2))
    }
}

impl<A, B, C> InternTo<(String, String, String), (A, B, C)> for Interner
    where
        Interner: InternTo<String, A>,
        Interner: InternTo<String, B>,
        Interner: InternTo<String, C>,
{
    fn intern(&mut self, (e1, e2, e3): (String, String, String)) -> (A, B, C) {
        (self.intern(e1), self.intern(e2), self.intern(e3))
    }
}

fn load_facts_from_file<T: DeserializeOwned>(facts_dir: &Path, facts_type: &str) -> Vec<T> {
    let filename = format!("{}.facts", facts_type);
    let facts_file = facts_dir.join(&filename);
    let mut reader = ReaderBuilder::new()
         .delimiter(b'\t')
         .has_headers(false)
         .from_path(facts_file)
         .unwrap();
    reader
        .deserialize()
        .map(|row| row.unwrap())
        .collect()
}

impl Interner {
    pub fn new() -> Self {
        Self {
            points: InternerTable::new(),
        }
    }
}

pub struct FactLoader {
    pub interner: Interner,
    pub facts: AllInputFacts,
}

impl FactLoader {
    pub fn new() -> Self {
        Self {
            interner: Interner::new(),
            facts: AllInputFacts::default(),
        }
    }
    pub fn load_all_facts(&mut self, facts_dir: &Path) {

        let facts = load_facts::<(String, String, String), _>(&mut self.interner, facts_dir, "borrow_region");
        self.facts.borrow_region.extend(facts);

        let facts = load_facts::<String, Region>(&mut self.interner, facts_dir, "universal_region");
        self.facts.universal_region.extend(facts);

        let facts = load_facts::<(String, String), _>(&mut self.interner, facts_dir, "cfg_edge");
        self.facts.cfg_edge.extend(facts);

        let facts = load_facts::<(String, String), _>(&mut self.interner, facts_dir, "killed");
        self.facts.killed.extend(facts);

        let facts = load_facts::<(String, String, String), _>(&mut self.interner, facts_dir, "outlives");
        self.facts.outlives.extend(facts);

        let facts = load_facts::<(String, String), _>(&mut self.interner, facts_dir, "region_live_at");
        self.facts.region_live_at.extend(facts);

        let facts = load_facts::<(String, String), _>(&mut self.interner, facts_dir, "invalidates");
        self.facts.invalidates.extend(facts);
    }
}

fn load_facts<F: DeserializeOwned, T>(interner: &mut Interner, facts_dir: &Path, facts_type: &str) -> Vec<T>
    where
        Interner: InternTo<F, T>
{
    load_facts_from_file(facts_dir, facts_type)
        .into_iter()
        .map(|fact| Interner::intern(interner, fact))
        .collect()
}

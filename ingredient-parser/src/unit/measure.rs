use crate::unit::singular;
use crate::unit::{kind::MeasureKind, Unit};
use crate::util::num_without_zeroes;
use crate::IngredientParser;
use anyhow::Result;
use petgraph::Graph;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use tracing::{debug, info};

type MeasureGraph = Graph<Unit, f64>;

pub fn make_graph(mappings: Vec<(Measure, Measure)>) -> MeasureGraph {
    let mut g = Graph::<Unit, f64>::new();

    for (mut m_a, mut m_b) in mappings.into_iter() {
        m_a = m_a.normalize();
        m_b = m_b.normalize();
        let n_a = g
            .node_indices()
            .find(|i| g[*i] == m_a.unit)
            .unwrap_or_else(|| g.add_node(m_a.unit.clone().normalize()));
        let n_b = g
            .node_indices()
            .find(|i| g[*i] == m_b.unit)
            .unwrap_or_else(|| g.add_node(m_b.unit.clone().normalize()));
        let _c1 = g.add_edge(n_a, n_b, m_b.value / m_a.value);
        let _c2 = g.add_edge(n_b, n_a, m_a.value / m_b.value);
    }
    g
}
pub fn print_graph(g: MeasureGraph) -> String {
    format!("{}", petgraph::dot::Dot::new(&g))
}

pub fn add_time_amounts(a: Vec<Measure>) -> Measure {
    let mut m = Measure::parse_str("0 seconds");
    for x in a.into_iter() {
        m = m.add(x).unwrap();
    }
    m.denormalize()
}

#[derive(Clone, PartialEq, PartialOrd, Debug, Serialize, Deserialize)]
pub struct Measure {
    unit: Unit,
    value: f64,
    upper_value: Option<f64>,
}

// multiplication factors
const TSP_TO_TBSP: f64 = 3.0;
const TSP_TO_FL_OZ: f64 = 2.0;
const G_TO_K: f64 = 1000.0;
const CUP_TO_QUART: f64 = 4.0;
const TSP_TO_CUP: f64 = 48.0;
const GRAM_TO_OZ: f64 = 28.3495;
const OZ_TO_LB: f64 = 16.0;
const CENTS_TO_DOLLAR: f64 = 100.0;
const SEC_TO_MIN: f64 = 60.0;
const SEC_TO_HOUR: f64 = 3600.0;
const SEC_TO_DAY: f64 = 86400.0;

impl Measure {
    pub fn new_with_upper(unit: Unit, value: f64, upper_value: Option<f64>) -> Measure {
        Measure {
            unit,
            value,
            upper_value,
        }
    }
    pub fn from_string(s: String) -> Measure {
        IngredientParser::new(false).must_parse_amount(s.as_str())[0].clone()
    }
    pub fn parse_str(s: &str) -> Measure {
        IngredientParser::new(false).must_parse_amount(s)[0].clone()
    }
    pub fn unit(&self) -> Unit {
        self.unit.clone()
    }
    pub fn values(&self) -> (f64, Option<f64>, String) {
        (self.value, self.upper_value, self.unit_as_string())
    }
    pub fn normalize(&self) -> Measure {
        let (unit, factor) = match &self.unit {
            Unit::Teaspoon
            | Unit::Milliliter
            | Unit::Gram
            | Unit::Cent
            | Unit::KCal
            | Unit::Farhenheit
            | Unit::Celcius // todo: convert to farhenheit?
            | Unit::Inch
            | Unit::Whole
            | Unit::Second => return self.clone(),
            Unit::Other(x) => {
                let x2 = x.clone();
                let u2 = singular(&x2);
                return Measure::new_with_upper(Unit::Other(u2), self.value, self.upper_value);
            }

            Unit::Kilogram => (Unit::Gram, G_TO_K),

            Unit::Ounce => (Unit::Gram, GRAM_TO_OZ),
            Unit::Pound => (Unit::Gram, GRAM_TO_OZ * OZ_TO_LB),

            Unit::Liter => (Unit::Milliliter, G_TO_K),

            Unit::Tablespoon => (Unit::Teaspoon, TSP_TO_TBSP),
            Unit::Cup => (Unit::Teaspoon, TSP_TO_CUP),
            Unit::Quart => (Unit::Teaspoon, CUP_TO_QUART * TSP_TO_CUP),
            Unit::FluidOunce => (Unit::Teaspoon, TSP_TO_FL_OZ),

            Unit::Dollar => (Unit::Cent, CENTS_TO_DOLLAR),
            Unit::Day => (Unit::Second, SEC_TO_DAY),
            Unit::Hour => (Unit::Second, SEC_TO_HOUR),
            Unit::Minute => (Unit::Second, SEC_TO_MIN),
        };

        Measure {
            unit,
            value: self.value * factor,
            upper_value: self.upper_value.map(|x| x * factor),
        }
    }
    pub fn add(&self, b: Measure) -> Result<Measure> {
        info!("adding {:?} to {:?}", self, b);

        if let MeasureKind::Other = b.kind().unwrap() {
            return Ok(self.clone());
        }

        if self.kind().unwrap() != b.kind().unwrap() {
            return Err(anyhow::anyhow!(
                "Cannot add measures of different kinds: {:#?} {:?}",
                self,
                b
            ));
        }
        let left = self.normalize();
        let right = b.normalize();

        Ok(Measure {
            unit: left.unit.clone(),
            value: left.value + right.value,
            upper_value: match (left.upper_value, right.upper_value) {
                (Some(a), Some(b)) => Some(a + b),
                (None, None) => None,
                (None, Some(b)) => Some(left.value + b),
                (Some(a), None) => Some(a + right.value),
            },
        })
    }
    pub fn parse_new(unit: &str, value: f64) -> Measure {
        Measure::from_parts(unit, value, None)
    }
    pub fn parse_new_with_upper(unit: &str, value: f64, upper: f64) -> Measure {
        Measure::from_parts(unit, value, Some(upper))
    }
    pub fn from_parts(unit: &str, value: f64, upper_value: Option<f64>) -> Measure {
        Measure {
            unit: Unit::from_str(singular(unit).as_ref()).unwrap(),
            value,
            upper_value,
        }
    }

    pub fn kind(&self) -> Result<MeasureKind> {
        match self.unit {
            Unit::Gram => Ok(MeasureKind::Weight),
            Unit::Cent => Ok(MeasureKind::Money),
            Unit::Teaspoon | Unit::Milliliter => Ok(MeasureKind::Volume),
            Unit::KCal => Ok(MeasureKind::Calories),
            Unit::Second => Ok(MeasureKind::Time),
            Unit::Farhenheit | Unit::Celcius => Ok(MeasureKind::Temperature), // todo: convert to farhenheit?
            Unit::Inch => Ok(MeasureKind::Length),
            Unit::Other(_) => Ok(MeasureKind::Other),
            Unit::Kilogram
            | Unit::Liter
            | Unit::Tablespoon
            | Unit::Cup
            | Unit::Quart
            | Unit::FluidOunce
            | Unit::Ounce
            | Unit::Pound
            | Unit::Dollar
            | Unit::Whole
            | Unit::Day
            | Unit::Minute
            | Unit::Hour => self.normalize().kind(),
        }
    }

    pub fn denormalize(self) -> Measure {
        let (u, f) = match self.unit {
            Unit::Gram => (Unit::Gram, 1.0),
            Unit::Milliliter => (Unit::Milliliter, 1.0),
            Unit::Teaspoon => match self.value {
                // only for these measurements to we convert to the best fit, others stay bare due to the nature of the values
                m if { m < 3.0 } => (Unit::Teaspoon, 1.0),
                m if { m < 12.0 } => (Unit::Tablespoon, TSP_TO_TBSP),
                m if { m < CUP_TO_QUART * TSP_TO_CUP } => (Unit::Cup, TSP_TO_CUP),
                _ => (Unit::Quart, CUP_TO_QUART * TSP_TO_CUP),
            },
            Unit::Cent => (Unit::Dollar, CENTS_TO_DOLLAR),
            Unit::KCal => (Unit::KCal, 1.0),
            Unit::Second => match self.value {
                // only for these measurements to we convert to the best fit, others stay bare due to the nature of the values
                m if { m < SEC_TO_MIN } => (Unit::Second, 1.0),
                m if { m < SEC_TO_HOUR } => (Unit::Minute, SEC_TO_MIN),
                m if { m < SEC_TO_DAY } => (Unit::Hour, SEC_TO_HOUR),
                _ => (Unit::Day, SEC_TO_DAY),
            },
            Unit::Inch => (Unit::Inch, 1.0),
            Unit::Other(o) => (Unit::Other(o), 1.0),
            Unit::Kilogram
            | Unit::Liter
            | Unit::Tablespoon
            | Unit::Cup
            | Unit::Quart
            | Unit::FluidOunce
            | Unit::Ounce
            | Unit::Pound
            | Unit::Dollar
            | Unit::Farhenheit
            | Unit::Celcius // todo: convert to farhenheit?
            | Unit::Whole
            | Unit::Minute
            | Unit::Hour
            | Unit::Day => return self,
        };
        Measure {
            unit: u,
            value: self.value / f,
            upper_value: self.upper_value.map(|x| x / f),
        }
    }

    #[tracing::instrument]
    pub fn convert_measure_via_mappings(
        &self,
        target: MeasureKind,
        mappings: Vec<(Measure, Measure)>,
    ) -> Option<Measure> {
        let g = make_graph(mappings);
        let input = self.normalize();
        let unit_a = input.unit.clone();
        let unit_b = target.unit();

        let n_a = g.node_indices().find(|i| g[*i] == unit_a)?;
        let n_b = g.node_indices().find(|i| g[*i] == unit_b)?;

        debug!("calculating {:?} to {:?}", n_a, n_b);
        if !petgraph::algo::has_path_connecting(&g, n_a, n_b, None) {
            debug!("convert failed for {:?}", input);
            return None;
        };

        let steps =
            petgraph::algo::astar(&g, n_a, |finish| finish == n_b, |e| *e.weight(), |_| 0.0)
                .unwrap()
                .1;
        let mut factor: f64 = 1.0;
        for x in 0..steps.len() - 1 {
            let edge = g
                .find_edge(*steps.get(x).unwrap(), *steps.get(x + 1).unwrap())
                .unwrap();
            factor *= g.edge_weight(edge).unwrap();
        }

        let result = Measure::new_with_upper(
            unit_b,
            (input.value * factor * 100.0).round() / 100.0,
            input
                .upper_value
                .map(|x| (x * factor * 100.0).round() / 100.0),
        );
        debug!("{:?} -> {:?} ({} hops)", input, result, steps.len());
        Some(result.denormalize())
    }
    fn unit_as_string(&self) -> String {
        let measure = self.clone(); //.denormalize();
        let mut s = singular(&measure.unit().to_str());
        if (measure.unit() == Unit::Cup || measure.unit() == Unit::Minute)
            && (self.value > 1.0 || self.upper_value.unwrap_or_default() > 1.0)
        {
            s.push('s');
        }
        s
    }
}

impl fmt::Display for Measure {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let measure = self.clone().denormalize();
        write!(f, "{}", num_without_zeroes(measure.value)).unwrap();
        if let Some(u) = measure.upper_value {
            if u != 0.0 {
                write!(f, " - {}", num_without_zeroes(u)).unwrap();
            }
        }
        write!(f, " {}", self.unit_as_string())
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    #[test]
    fn test_measure() {
        let m1 = Measure::parse_str("16 tbsp");
        assert_eq!(
            m1.normalize(),
            Measure::new_with_upper(Unit::Teaspoon, 48.0, None)
        );
        assert_eq!(m1.normalize(), Measure::parse_new("cup", 1.0).normalize());
        assert_eq!(
            Measure::parse_str("25.2 grams").denormalize(),
            Measure::parse_new("g", 25.2)
        );
        assert_eq!(
            Measure::parse_str("2500.2 grams").denormalize(),
            Measure::parse_new("g", 2500.2)
        );
        assert_eq!(
            Measure::parse_str("12 foo").denormalize(),
            Measure::parse_new("whole", 12.0)
        );
    }

    #[test]
    fn test_convert() {
        let m = Measure::parse_str("1 tbsp");
        let tbsp_dollars = (
            Measure::parse_str("2 tbsp"),
            Measure::parse_str("4 dollars"),
        );
        assert_eq!(
            Measure::parse_str("2 dollars"),
            m.convert_measure_via_mappings(MeasureKind::Money, vec![tbsp_dollars.clone()])
                .unwrap()
        );

        assert!(m
            .convert_measure_via_mappings(MeasureKind::Volume, vec![tbsp_dollars])
            .is_none());
    }
    #[test]
    fn test_convert_lb() {
        let grams_dollars = (Measure::parse_str("1 gram"), Measure::parse_str("1 dollar"));
        assert_eq!(
            Measure::parse_str("2 dollars"),
            Measure::parse_str("2 grams")
                .convert_measure_via_mappings(MeasureKind::Money, vec![grams_dollars.clone()])
                .unwrap()
        );
        assert_eq!(
            Measure::parse_str("56.699 dollars"),
            Measure::parse_str("2 oz")
                .convert_measure_via_mappings(MeasureKind::Money, vec![grams_dollars.clone()])
                .unwrap()
        );
        assert_eq!(
            Measure::parse_str("226.796 dollars"),
            Measure::parse_str(".5 lb")
                .convert_measure_via_mappings(MeasureKind::Money, vec![grams_dollars.clone()])
                .unwrap()
        );
        assert_eq!(
            Measure::parse_str("453.592 dollars"),
            Measure::parse_str("1 lb")
                .convert_measure_via_mappings(MeasureKind::Money, vec![grams_dollars])
                .unwrap()
        );
    }
    #[test]
    fn test_convert_other() {
        assert_eq!(
            Measure::parse_str("10.0 cents").denormalize(),
            Measure::parse_str("1 whole")
                .convert_measure_via_mappings(
                    MeasureKind::Money,
                    vec![(
                        Measure::parse_str("12 whole"),
                        Measure::parse_str("1.20 dollar"),
                    )]
                )
                .unwrap()
        );
    }
    #[test]
    fn test_convert_range() {
        assert_eq!(
            Measure::parse_str("5-10 dollars"),
            Measure::parse_str("1-2 whole")
                .convert_measure_via_mappings(
                    MeasureKind::Money,
                    vec![(
                        Measure::parse_str("4 whole"),
                        Measure::parse_str("20 dollar")
                    )]
                )
                .unwrap()
        );
    }
    #[test]
    fn test_convert_transitive() {
        assert_eq!(
            Measure::parse_str("1 cent").denormalize(),
            Measure::parse_str("1 grams")
                .convert_measure_via_mappings(
                    MeasureKind::Money,
                    vec![
                        (Measure::parse_str("1 cent"), Measure::parse_str("1 tsp"),),
                        (Measure::parse_str("1 grams"), Measure::parse_str("1 tsp"),),
                    ]
                )
                .unwrap()
        );
        assert_eq!(
            Measure::parse_str("1 dollar"),
            Measure::parse_str("1 grams")
                .convert_measure_via_mappings(
                    MeasureKind::Money,
                    vec![
                        (Measure::parse_str("1 dollar"), Measure::parse_str("1 cup"),),
                        (Measure::parse_str("1 grams"), Measure::parse_str("1 cup"),),
                    ]
                )
                .unwrap()
        );
    }
    #[test]
    fn test_convert_kcal() {
        assert_eq!(
            Measure::parse_str("200 kcal"),
            Measure::parse_str("100 g")
                .convert_measure_via_mappings(
                    MeasureKind::Calories,
                    vec![
                        (
                            Measure::parse_str("20 cups"),
                            Measure::parse_str("40 grams"),
                        ),
                        (
                            Measure::parse_str("20 grams"),
                            Measure::parse_str("40 kcal"),
                        )
                    ]
                )
                .unwrap()
        );
    }
    #[test]
    fn test_add() {
        assert_eq!(
            add_time_amounts(vec![
                Measure::parse_str("2-3 minutes"),
                Measure::parse_str("10 minutes")
            ]),
            Measure::parse_str("12-13 minutes"),
        );
    }
    #[test]
    fn test_print_graph() {
        let g = make_graph(vec![
            (
                Measure::parse_str("1 tbsp"),
                Measure::parse_str("30 dollar"),
            ),
            (Measure::parse_str("1 tsp"), Measure::parse_str("1 gram")),
        ]);
        assert_eq!(
            print_graph(g),
            r#"digraph {
    0 [ label = "Teaspoon" ]
    1 [ label = "Cent" ]
    2 [ label = "Gram" ]
    0 -> 1 [ label = "1000" ]
    1 -> 0 [ label = "0.001" ]
    0 -> 2 [ label = "1" ]
    2 -> 0 [ label = "1" ]
}
"#
        );
    }
    #[test]
    fn test_singular_plural() {
        assert_eq!(Measure::parse_str("1 cup").unit_as_string(), "cup");
        assert_eq!(Measure::parse_str("2 cup").unit_as_string(), "cups");
        assert_eq!(Measure::parse_str("3 grams").unit_as_string(), "g");
    }
}

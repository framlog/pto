#![feature(iterator_try_collect)]
#![feature(btree_cursors)]

use std::{collections::BTreeMap, path::PathBuf};

use anyhow::{anyhow, Result};
use clap::Parser;

/// Personal Tax Optimizer. It tries to find the optimal movement to minimize your tax payment.
#[derive(Parser)]
struct Args {
    /// Input your case in a comma delimited format: monthly_salary,monthly_tax_deduction,
    /// year_bonus.
    #[arg(short, long, value_parser=parse_record)]
    record: Record,
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,
}

fn parse_record(arg: &str) -> Result<Record> {
    let tokens: Vec<_> = arg.split(',').map(|s| s.parse::<f64>()).try_collect()?;
    Ok(Record {
        monthly_salary: tokens[0],
        monthly_tax_deduction: tokens[1],
        year_bonus: tokens[2],
        movement: 0.0,
    })
}

#[derive(Clone)]
struct Record {
    monthly_salary: f64,
    monthly_tax_deduction: f64,
    year_bonus: f64,
    movement: f64,
}

impl Record {
    fn adjust(&mut self, budget: f64) -> Result<()> {
        let budget = self.year_bonus.min(budget);
        anyhow::ensure!(budget > 0.0, "budget is invalid");
        self.year_bonus -= budget;
        self.movement += budget;
        Ok(())
    }
}

struct Tax {
    salary: f64,
    year_bonus: f64,
}

impl std::fmt::Display for Tax {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let total = self.salary + self.year_bonus;
        f.write_fmt(format_args!(
            "{total} (tax for salary: {}, tax for year bonus: {})",
            self.salary, self.year_bonus
        ))
    }
}

impl Tax {
    fn total(&self) -> f64 {
        self.salary + self.year_bonus
    }
}

struct TaxConfig {
    salary: BTreeMap<i32, f64>,
    year_bonus: BTreeMap<i32, f64>,
}

impl TryFrom<toml::Table> for TaxConfig {
    type Error = anyhow::Error;

    fn try_from(tbl: toml::Table) -> Result<Self> {
        let parse = |name: &str| -> Result<BTreeMap<i32, f64>> {
            let mut ret = BTreeMap::new();
            for r in tbl[name]["rule"]
                .as_array()
                .ok_or_else(|| anyhow!("rule is not an array"))?
            {
                ret.insert(
                    r["bound"]
                        .as_integer()
                        .map(|v| v as i32)
                        .ok_or_else(|| anyhow!("missing bound"))?,
                    r["ratio"]
                        .as_float()
                        .ok_or_else(|| anyhow!("missing ratio"))?,
                );
            }
            Ok(ret)
        };
        Ok(Self {
            salary: parse("salary")?,
            year_bonus: parse("year_bonus")?,
        })
    }
}

impl TaxConfig {
    /// Caluculate the tax for the given record. Return tax for salary and tax for year bouns in
    /// tuple format.
    fn calc(&self, r: &Record) -> Tax {
        let total_salary = r.movement + 0f64.max(r.monthly_salary - r.monthly_tax_deduction) * 12.0;
        let mut salary_tax = 0.0;
        let mut last = 0.0;
        for (rb, ratio) in &self.salary {
            let budget = (*rb as f64).min(total_salary) - last;
            salary_tax += budget * ratio;
            if *rb as f64 >= total_salary {
                break;
            }
            last = *rb as f64;
        }
        let cursor = self.year_bonus.lower_bound(std::ops::Bound::Included(
            &((r.year_bonus / 12.0).ceil() as i32),
        ));
        let ratio = cursor.peek_next().unwrap().1;
        let bonus_tax = ratio * r.year_bonus;
        Tax {
            salary: salary_tax,
            year_bonus: bonus_tax,
        }
    }
}

const DEFAULT_CONFIG_FILE_PATH: &str = "./config.toml";

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let raw_config: toml::Table = toml::from_str(
        &tokio::fs::read_to_string(args.config.unwrap_or(DEFAULT_CONFIG_FILE_PATH.into())).await?,
    )?;
    let tax_config = TaxConfig::try_from(raw_config)?;
    let mut payment = tax_config.calc(&args.record);

    println!("Before: {payment}");

    let mut r = args.record;
    let mut movement = 0.0;
    while r.year_bonus > 0.0 {
        r.adjust(10.0)?;
        let v = tax_config.calc(&r);
        if v.total() < payment.total() {
            payment = v;
            movement = r.movement;
        }
    }

    println!("After: {payment}\nMovement: {movement}");
    Ok(())
}

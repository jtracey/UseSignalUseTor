// Representations of Distributions for sampling timing and message sizes.

use rand_distr::{
    Bernoulli, BernoulliError, Binomial, BinomialError, Distribution, Exp, ExpError, GeoError,
    Geometric, HyperGeoError, Hypergeometric, LogNormal, Normal, NormalError, Pareto, ParetoError,
    Poisson, PoissonError, Uniform, WeightedAliasIndex, WeightedError,
};
use rand_xoshiro::Xoshiro256PlusPlus;
use serde::Deserialize;
use std::str::FromStr;
use tokio::time::Duration;

/// The set of Distributions we currently support for message sizes (in padding blocks).
/// To modify the code to add support for more, one approach is to first add them here,
/// then fix all the compiler errors and warnings that arise as a result.
#[derive(Clone, Debug)]
pub enum MessageDistribution {
    // Poisson is only defined for floats for technical reasons.
    // https://rust-random.github.io/book/guide-dist.html#integers
    Poisson(Poisson<f64>),
    Binomial(Binomial),
    Geometric(Geometric),
    Hypergeometric(Hypergeometric),
    Weighted(WeightedAliasIndex<u32>, Vec<u32>),
}

impl Distribution<u32> for MessageDistribution {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> u32 {
        let ret = match self {
            Self::Poisson(d) => d.sample(rng) as u64,
            Self::Binomial(d) => d.sample(rng),
            Self::Geometric(d) => d.sample(rng),
            Self::Hypergeometric(d) => d.sample(rng),
            Self::Weighted(d, v) => v[d.sample(rng)].into(),
        };
        std::cmp::min(ret, mgen::MAX_BLOCKS_IN_BODY.into()) as u32
    }
}

/// The set of Distributions we currently support for timings.
/// To modify the code to add support for more, one approach is to first add them here,
/// then fix all the compiler errors and warnings that arise as a result.
#[derive(Clone, Debug)]
pub enum TimingDistribution {
    Normal(Normal<f64>),
    LogNormal(LogNormal<f64>),
    Uniform(Uniform<f64>),
    Exp(Exp<f64>),
    Pareto(Pareto<f64>),
    Weighted(WeightedAliasIndex<u32>, Vec<f64>),
}

impl Distribution<f64> for TimingDistribution {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> f64 {
        let ret = match self {
            Self::Normal(d) => d.sample(rng),
            Self::LogNormal(d) => d.sample(rng),
            Self::Uniform(d) => d.sample(rng),
            Self::Exp(d) => d.sample(rng),
            Self::Pareto(d) => d.sample(rng),
            Self::Weighted(d, v) => v[d.sample(rng)],
        };
        ret.max(0.0)
    }
}

/// The set of distributions necessary to represent the actions of the state machine.
#[derive(Clone, Debug)]
pub struct Distributions {
    pub m: MessageDistribution,
    pub i: TimingDistribution,
    pub w: TimingDistribution,
    pub a_s: TimingDistribution,
    pub a_r: TimingDistribution,
    pub s: Bernoulli,
    pub r: Bernoulli,
}

impl TimingDistribution {
    pub fn sample_secs(&self, rng: &mut Xoshiro256PlusPlus) -> Duration {
        Duration::from_secs_f64(self.sample(rng))
    }
}

/// The same as Distributions, but designed for easier deserialization.
#[derive(Clone, Debug, Deserialize)]
pub struct ConfigDistributions {
    m: ConfigMessageDistribution,
    i: ConfigTimingDistribution,
    w: ConfigTimingDistribution,
    a_s: ConfigTimingDistribution,
    a_r: ConfigTimingDistribution,
    s: f64,
    r: f64,
}

/// The same as MessageDistribution, but designed for easier deserialization.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "distribution")]
enum ConfigMessageDistribution {
    Poisson {
        lambda: f64,
    },
    Binomial {
        n: u64,
        p: f64,
    },
    Geometric {
        p: f64,
    },
    Hypergeometric {
        total_population_size: u64,
        population_with_feature: u64,
        sample_size: u64,
    },
    Weighted {
        weights_file: String,
    },
}

/// The same as TimingDistribution, but designed for easier deserialization.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "distribution")]
enum ConfigTimingDistribution {
    Normal { mean: f64, std_dev: f64 },
    LogNormal { mean: f64, std_dev: f64 },
    Uniform { low: f64, high: f64 },
    Exp { lambda: f64 },
    Pareto { scale: f64, shape: f64 },
    Weighted { weights_file: String },
}

#[derive(Debug)]
pub enum DistParameterError {
    Poisson(PoissonError),
    Binomial(BinomialError),
    Geometric(GeoError),
    Hypergeometric(HyperGeoError),
    Bernoulli(BernoulliError),
    Normal(NormalError),
    LogNormal(NormalError),
    Uniform, // Uniform::new doesn't return an error, it just panics
    Exp(ExpError),
    Pareto(ParetoError),
    WeightedParseError(WeightedParseError),
}

impl std::fmt::Display for DistParameterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for DistParameterError {}

#[derive(Debug)]
pub enum WeightedParseError {
    WeightedError(WeightedError),
    Io(std::io::Error),
    ParseNumError,
}

impl std::fmt::Display for WeightedParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for WeightedParseError {}

fn parse_weights_file<T: FromStr>(
    path: String,
) -> Result<(WeightedAliasIndex<u32>, Vec<T>), WeightedParseError> {
    let weights_file = std::fs::read_to_string(path).map_err(WeightedParseError::Io)?;
    let mut weights_lines = weights_file.lines();
    let weights = weights_lines
        .next()
        .unwrap()
        .split(',')
        .map(u32::from_str)
        .collect::<Result<Vec<_>, _>>()
        .or(Err(WeightedParseError::ParseNumError))?;
    let vals = weights_lines
        .next()
        .expect("Weights file only has one line")
        .split(',')
        .map(T::from_str)
        .collect::<Result<Vec<_>, _>>()
        .or(Err(WeightedParseError::ParseNumError))?;
    assert!(
        weights.len() == vals.len(),
        "Weights file doesn't have the same number of weights and values."
    );
    let dist =
        WeightedAliasIndex::<u32>::new(weights).map_err(WeightedParseError::WeightedError)?;
    Ok((dist, vals))
}

impl TryFrom<ConfigMessageDistribution> for MessageDistribution {
    type Error = DistParameterError;

    fn try_from(dist: ConfigMessageDistribution) -> Result<Self, DistParameterError> {
        let dist = match dist {
            ConfigMessageDistribution::Poisson { lambda } => MessageDistribution::Poisson(
                Poisson::new(lambda).map_err(DistParameterError::Poisson)?,
            ),
            ConfigMessageDistribution::Binomial { n, p } => MessageDistribution::Binomial(
                Binomial::new(n, p).map_err(DistParameterError::Binomial)?,
            ),
            ConfigMessageDistribution::Geometric { p } => MessageDistribution::Geometric(
                Geometric::new(p).map_err(DistParameterError::Geometric)?,
            ),
            ConfigMessageDistribution::Hypergeometric {
                total_population_size,
                population_with_feature,
                sample_size,
            } => MessageDistribution::Hypergeometric(
                Hypergeometric::new(total_population_size, population_with_feature, sample_size)
                    .map_err(DistParameterError::Hypergeometric)?,
            ),
            ConfigMessageDistribution::Weighted { weights_file } => {
                let (dist, vals) = parse_weights_file(weights_file)
                    .map_err(DistParameterError::WeightedParseError)?;
                MessageDistribution::Weighted(dist, vals)
            }
        };
        Ok(dist)
    }
}

impl TryFrom<ConfigTimingDistribution> for TimingDistribution {
    type Error = DistParameterError;

    fn try_from(dist: ConfigTimingDistribution) -> Result<Self, DistParameterError> {
        let dist = match dist {
            ConfigTimingDistribution::Normal { mean, std_dev } => TimingDistribution::Normal(
                Normal::new(mean, std_dev).map_err(DistParameterError::Normal)?,
            ),
            ConfigTimingDistribution::LogNormal { mean, std_dev } => TimingDistribution::LogNormal(
                LogNormal::new(mean, std_dev).map_err(DistParameterError::LogNormal)?,
            ),
            ConfigTimingDistribution::Uniform { low, high } => {
                if low >= high {
                    return Err(DistParameterError::Uniform);
                }
                TimingDistribution::Uniform(Uniform::new(low, high))
            }
            ConfigTimingDistribution::Exp { lambda } => {
                TimingDistribution::Exp(Exp::new(lambda).map_err(DistParameterError::Exp)?)
            }
            ConfigTimingDistribution::Pareto { scale, shape } => TimingDistribution::Pareto(
                Pareto::new(scale, shape).map_err(DistParameterError::Pareto)?,
            ),
            ConfigTimingDistribution::Weighted { weights_file } => {
                let (dist, vals) = parse_weights_file(weights_file)
                    .map_err(DistParameterError::WeightedParseError)?;
                TimingDistribution::Weighted(dist, vals)
            }
        };
        Ok(dist)
    }
}

impl TryFrom<ConfigDistributions> for Distributions {
    type Error = DistParameterError;

    fn try_from(config: ConfigDistributions) -> Result<Self, DistParameterError> {
        Ok(Distributions {
            m: config.m.try_into()?,
            i: config.i.try_into()?,
            w: config.w.try_into()?,
            a_s: config.a_s.try_into()?,
            a_r: config.a_r.try_into()?,
            s: Bernoulli::new(config.s).map_err(DistParameterError::Bernoulli)?,
            r: Bernoulli::new(config.r).map_err(DistParameterError::Bernoulli)?,
        })
    }
}

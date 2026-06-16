use serde::Deserialize;

use super::WeatherError;

const ENDPOINT: &str = "https://geocoding-api.open-meteo.com/v1/search";

/// How many candidates to fetch per query. More than fit the picker on screen
/// at once, on purpose: common ambiguous names (Springfield, San Jose) need a
/// list the user can scroll, and `rank` orders the whole set by population.
const CANDIDATE_LIMIT: u32 = 20;

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct GeoResult {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub timezone: String,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub admin1: Option<String>,
    #[serde(default)]
    pub elevation: Option<f64>,
    #[serde(default)]
    pub population: Option<u64>,
}

impl GeoResult {
    pub fn label(&self) -> String {
        let mut parts = vec![self.name.clone()];
        if let Some(admin) = &self.admin1
            && admin != &self.name
        {
            parts.push(admin.clone());
        }
        if let Some(country) = &self.country {
            parts.push(country.clone());
        }
        parts.join(", ")
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct GeoResponse {
    #[serde(default)]
    pub results: Vec<GeoResult>,
}

/// Rank candidates most-likely-first. Open-Meteo returns its own relevance
/// order; we resort by population so a one-word query like "capetown" lands on
/// the real city, not a hamlet that merely matches the spelling. When
/// population data is absent the stable index tie-break keeps the API's
/// relevance order, so an unranked query never regresses.
pub fn rank(results: Vec<GeoResult>) -> Vec<GeoResult> {
    let mut indexed: Vec<(usize, GeoResult)> = results.into_iter().enumerate().collect();
    indexed.sort_by(|(ai, a), (bi, b)| {
        b.population
            .unwrap_or(0)
            .cmp(&a.population.unwrap_or(0))
            .then(ai.cmp(bi))
    });
    indexed.into_iter().map(|(_, r)| r).collect()
}

/// The single most likely match: the top of [`rank`]. Every non-interactive
/// resolver path (CLI flag, saved config, piped output) uses this so they all
/// agree with the picker's default-selected row.
pub fn best_match(results: Vec<GeoResult>) -> Option<GeoResult> {
    rank(results).into_iter().next()
}

pub fn geocode(query: &str) -> Result<Vec<GeoResult>, WeatherError> {
    let count = CANDIDATE_LIMIT.to_string();
    let mut response = super::AGENT
        .get(ENDPOINT)
        .query("name", query)
        .query("count", count.as_str())
        .query("language", "en")
        .query("format", "json")
        .call()?;
    let status = response.status();
    if !status.is_success() {
        let body = response.body_mut().read_to_string().unwrap_or_default();
        return Err(WeatherError::Http {
            status: status.as_u16(),
            body,
        });
    }
    let body: GeoResponse = response.body_mut().read_json()?;
    Ok(body.results)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geo(name: &str, population: Option<u64>) -> GeoResult {
        GeoResult {
            name: name.to_string(),
            latitude: 0.0,
            longitude: 0.0,
            timezone: "UTC".to_string(),
            country: None,
            admin1: None,
            elevation: None,
            population,
        }
    }

    #[test]
    fn rank_orders_by_population_desc() {
        let ranked = rank(vec![
            geo("Capetown", Some(1_000)),
            geo("Cape Town", Some(3_400_000)),
            geo("Capetown hamlet", None),
        ]);
        let names: Vec<_> = ranked.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["Cape Town", "Capetown", "Capetown hamlet"]);
    }

    #[test]
    fn rank_without_population_keeps_api_order() {
        // All None means every key is 0, so the stable index tie-break wins and
        // the API's relevance order survives untouched.
        let ranked = rank(vec![
            geo("First", None),
            geo("Second", None),
            geo("Third", None),
        ]);
        let names: Vec<_> = ranked.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["First", "Second", "Third"]);
    }

    #[test]
    fn best_match_takes_the_most_populous() {
        let pick = best_match(vec![geo("Small", Some(2_000)), geo("Big", Some(900_000))]);
        assert_eq!(pick.unwrap().name, "Big");
    }

    #[test]
    fn best_match_of_empty_is_none() {
        assert!(best_match(vec![]).is_none());
    }
}

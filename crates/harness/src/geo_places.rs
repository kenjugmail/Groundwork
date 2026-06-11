//! Place-name → county GEOID for the sensing metro. The agent reports
//! geography exactly as written; this table resolves it. Unresolvable
//! geo_text drops the signal (and counts toward the geo-resolution gate).

use adapters::geo;

/// NYC neighborhoods/boroughs + Westchester municipalities → county FIPS.
/// Lowercased keys; not exhaustive — extending it is a PR.
const PLACES: &[(&str, &str)] = &[
    // Bronx (005)
    ("the bronx", "005"), ("south bronx", "005"), ("hunts point", "005"),
    ("mott haven", "005"), ("fordham", "005"), ("norwood", "005"),
    ("tremont", "005"), ("morrisania", "005"), ("riverdale", "005"),
    ("soundview", "005"), ("kingsbridge", "005"), ("belmont", "005"),
    // Brooklyn (047)
    ("brownsville", "047"), ("east new york", "047"), ("bushwick", "047"),
    ("bedford-stuyvesant", "047"), ("bed-stuy", "047"), ("sunset park", "047"),
    ("coney island", "047"), ("flatbush", "047"), ("canarsie", "047"),
    ("williamsburg", "047"), ("crown heights", "047"), ("borough park", "047"),
    // Manhattan (061)
    ("harlem", "061"), ("east harlem", "061"), ("washington heights", "061"),
    ("inwood", "061"), ("lower east side", "061"), ("chinatown", "061"),
    ("midtown", "061"), ("upper west side", "061"), ("upper east side", "061"),
    // Queens (081)
    ("astoria", "081"), ("jackson heights", "081"), ("corona", "081"),
    ("elmhurst", "081"), ("flushing", "081"), ("jamaica", "081"),
    ("far rockaway", "081"), ("the rockaways", "081"), ("ridgewood", "081"),
    ("long island city", "081"), ("woodside", "081"),
    // Staten Island (085)
    ("st. george", "085"), ("stapleton", "085"), ("port richmond", "085"),
    ("tottenville", "085"),
    // Westchester (119)
    ("yonkers", "119"), ("mount vernon", "119"), ("new rochelle", "119"),
    ("white plains", "119"), ("peekskill", "119"), ("ossining", "119"),
    ("port chester", "119"), ("tarrytown", "119"), ("sleepy hollow", "119"),
    ("elmsford", "119"), ("greenburgh", "119"), ("yorktown", "119"),
    ("mamaroneck", "119"), ("harrison", "119"), ("rye", "119"),
    ("scarsdale", "119"), ("dobbs ferry", "119"), ("hastings-on-hudson", "119"),
    ("croton-on-hudson", "119"), ("cortlandt", "119"), ("somers", "119"),
    ("bedford", "119"), ("mount kisco", "119"), ("pleasantville", "119"),
];

/// Resolve free-text place name → county GEOID (e.g. "36119").
/// Tries the place table, then the NY county-name resolver, restricted to
/// the sensing metro (signals outside it are out of scope for the harness).
pub fn resolve(geo_text: &str) -> Option<String> {
    let t = geo_text.to_lowercase();
    let t = t.trim().trim_start_matches("the ").trim();
    for (place, fips) in PLACES {
        let p = place.trim_start_matches("the ");
        if t == p || t.contains(p) {
            return Some(format!("36{fips}"));
        }
    }
    if let Some(geoid) = geo::ny_county_geoid(geo_text) {
        if geo::in_scope(&geoid[2..]) {
            return Some(geoid);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_places_and_boroughs() {
        assert_eq!(resolve("Mount Vernon").as_deref(), Some("36119"));
        assert_eq!(resolve("Astoria").as_deref(), Some("36081"));
        assert_eq!(resolve("the Bronx").as_deref(), Some("36005"));
        assert_eq!(resolve("South Bronx").as_deref(), Some("36005"));
        assert_eq!(resolve("Brooklyn").as_deref(), Some("36047"));
        assert_eq!(resolve("Buffalo"), None); // out of metro scope
        assert_eq!(resolve("Narnia"), None);
    }
}

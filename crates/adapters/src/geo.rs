//! County-name → FIPS resolution for New York State.
//! Coarse geo-resolution used by adapters whose sources name counties in prose.

/// NY state FIPS.
pub const NY_STATE_FIPS: &str = "36";

/// Counties in v0 scope: the five NYC boroughs + Westchester.
pub const SCOPE_COUNTY_FIPS: [&str; 6] = ["005", "047", "061", "081", "085", "119"];

/// All 62 NY counties as (fips, name), for registering geo_units.
pub const NY_COUNTIES: [(&str, &str); 62] = [
    ("001", "Albany"), ("003", "Allegany"), ("005", "Bronx"), ("007", "Broome"),
    ("009", "Cattaraugus"), ("011", "Cayuga"), ("013", "Chautauqua"), ("015", "Chemung"),
    ("017", "Chenango"), ("019", "Clinton"), ("021", "Columbia"), ("023", "Cortland"),
    ("025", "Delaware"), ("027", "Dutchess"), ("029", "Erie"), ("031", "Essex"),
    ("033", "Franklin"), ("035", "Fulton"), ("037", "Genesee"), ("039", "Greene"),
    ("041", "Hamilton"), ("043", "Herkimer"), ("045", "Jefferson"), ("047", "Kings"),
    ("049", "Lewis"), ("051", "Livingston"), ("053", "Madison"), ("055", "Monroe"),
    ("057", "Montgomery"), ("059", "Nassau"), ("061", "New York"), ("063", "Niagara"),
    ("065", "Oneida"), ("067", "Onondaga"), ("069", "Ontario"), ("071", "Orange"),
    ("073", "Orleans"), ("075", "Oswego"), ("077", "Otsego"), ("079", "Putnam"),
    ("081", "Queens"), ("083", "Rensselaer"), ("085", "Richmond"), ("087", "Rockland"),
    ("089", "St. Lawrence"), ("091", "Saratoga"), ("093", "Schenectady"), ("095", "Schoharie"),
    ("097", "Schuyler"), ("099", "Seneca"), ("101", "Steuben"), ("103", "Suffolk"),
    ("105", "Sullivan"), ("107", "Tioga"), ("109", "Tompkins"), ("111", "Ulster"),
    ("113", "Warren"), ("115", "Washington"), ("117", "Wayne"), ("119", "Westchester"),
    ("121", "Wyoming"), ("123", "Yates"),
];

/// All 62 NY counties, name → 3-digit county FIPS.
/// Names normalized: lowercase, no "county" suffix.
pub fn ny_county_fips(name: &str) -> Option<&'static str> {
    // NYC Open Data alone mixes "Staten Island" and "Staten_Island";
    // normalize separators and punctuation before matching.
    let n = name
        .to_lowercase()
        .replace("county", "")
        .replace(['.', '_'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let fips = match n.as_str() {
        "albany" => "001", "allegany" => "003", "bronx" => "005", "broome" => "007",
        "cattaraugus" => "009", "cayuga" => "011", "chautauqua" => "013", "chemung" => "015",
        "chenango" => "017", "clinton" => "019", "columbia" => "021", "cortland" => "023",
        "delaware" => "025", "dutchess" => "027", "erie" => "029", "essex" => "031",
        "franklin" => "033", "fulton" => "035", "genesee" => "037", "greene" => "039",
        "hamilton" => "041", "herkimer" => "043", "jefferson" => "045",
        "kings" | "brooklyn" => "047", "lewis" => "049", "livingston" => "051",
        "madison" => "053", "monroe" => "055", "montgomery" => "057", "nassau" => "059",
        "new york" | "manhattan" => "061", "niagara" => "063", "oneida" => "065",
        "onondaga" => "067", "ontario" => "069", "orange" => "071", "orleans" => "073",
        "oswego" => "075", "otsego" => "077", "putnam" => "079",
        "queens" => "081", "rensselaer" => "083", "richmond" | "staten island" => "085",
        "rockland" => "087", "saratoga" => "091", "schenectady" => "093",
        "schoharie" => "095", "schuyler" => "097", "seneca" => "099",
        "st lawrence" | "saint lawrence" => "089", "steuben" => "101", "suffolk" => "103",
        "sullivan" => "105", "tioga" => "107", "tompkins" => "109", "ulster" => "111",
        "warren" => "113", "washington" => "115", "wayne" => "117",
        "westchester" => "119", "wyoming" => "121", "yates" => "123",
        _ => return None,
    };
    Some(fips)
}

/// 5-digit county GEOID for an NY county name, if recognized.
pub fn ny_county_geoid(name: &str) -> Option<String> {
    ny_county_fips(name).map(|f| format!("{NY_STATE_FIPS}{f}"))
}

pub fn in_scope(county_fips: &str) -> bool {
    SCOPE_COUNTY_FIPS.contains(&county_fips)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_boroughs_and_aliases() {
        assert_eq!(ny_county_geoid("Westchester County").as_deref(), Some("36119"));
        assert_eq!(ny_county_geoid("Brooklyn").as_deref(), Some("36047"));
        assert_eq!(ny_county_geoid("New York").as_deref(), Some("36061"));
        assert_eq!(ny_county_geoid("Narnia"), None);
    }
}

use crate::{Result, WynError};

pub const SATOSHIS_PER_WYN: u64 = 100_000_000;

pub fn parse_wyn(value: &str) -> Result<u64> {
    let value = value.trim();
    if value.is_empty() || value.starts_with('-') {
        return Err(WynError::Validation("valor monetário inválido".into()));
    }

    let mut parts = value.split('.');
    let whole = parts
        .next()
        .ok_or_else(|| WynError::Validation("valor monetário inválido".into()))?;
    let fraction = parts.next().unwrap_or("");

    if parts.next().is_some() || fraction.len() > 8 {
        return Err(WynError::Validation(
            "use no máximo 8 casas decimais".into(),
        ));
    }

    let whole_value = if whole.is_empty() {
        0
    } else {
        whole
            .parse::<u64>()
            .map_err(|_| WynError::Validation("valor monetário inválido".into()))?
    };

    let mut fraction_string = fraction.to_string();
    while fraction_string.len() < 8 {
        fraction_string.push('0');
    }

    let fraction_value = if fraction_string.is_empty() {
        0
    } else {
        fraction_string
            .parse::<u64>()
            .map_err(|_| WynError::Validation("valor monetário inválido".into()))?
    };

    whole_value
        .checked_mul(SATOSHIS_PER_WYN)
        .and_then(|v| v.checked_add(fraction_value))
        .ok_or_else(|| WynError::Validation("valor monetário excede o limite".into()))
}

pub fn format_wyn(satoshis: u64) -> String {
    let whole = satoshis / SATOSHIS_PER_WYN;
    let fraction = satoshis % SATOSHIS_PER_WYN;
    format!("{whole}.{fraction:08}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_formats_values() {
        assert_eq!(parse_wyn("1").unwrap(), 100_000_000);
        assert_eq!(parse_wyn("1.5").unwrap(), 150_000_000);
        assert_eq!(format_wyn(150_000_000), "1.50000000");
    }
}

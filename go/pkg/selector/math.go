package selector

import (
	"math"
	"strings"
	"time"
)

var ABKeys = []string{
	"ab_acoustic",
	"ab_aggressive",
	"ab_bright",
	"ab_danceable",
	"ab_female",
	"ab_happy",
	"ab_instrumental",
	"ab_party",
	"ab_relaxed",
	"ab_sad",
	"ab_tonal",
}

func RotationToSeconds(rv float64) float64 {
	return math.Pow(10.0, rv) * 3600.0
}

func CalculateRestraintWeight(restraint float64) float64 {
	return math.Pow(10.0, -restraint)
}

func CalculateRecoveryWeight(ageSec, rotSec, recSec float64) float64 {
	if ageSec <= rotSec {
		return 0.0
	}
	if ageSec >= (rotSec+recSec) || recSec <= 0 {
		return 1.0
	}
	return (ageSec - rotSec) / recSec
}

func CalculateOccasionWeight(occasions string, nowSec float64) float64 {
	if occasions == "" {
		return 1.0
	}

	t := time.Unix(int64(nowSec), 0).UTC()
	month := t.Month()
	day := t.Day()

	w := 1.0
	occUpper := strings.ToUpper(occasions)

	if strings.Contains(occUpper, "[C]") { // Christmas
		if month == time.December {
			if day >= 20 && day <= 26 {
				w *= 10.0
			} else {
				w *= 3.0
			}
		} else if month == time.November && day >= 25 {
			w *= 1.5
		} else {
			w *= 0.0001 // Hard lockout outside holiday season
		}
	}

	if strings.Contains(occUpper, "[W]") { // Winter
		if month == time.December || month == time.January || month == time.February {
			w *= 1.5
		} else {
			w *= 0.5
		}
	}

	if strings.Contains(occUpper, "[S]") { // Summer
		if month == time.June || month == time.July || month == time.August {
			w *= 1.5
		} else {
			w *= 0.5
		}
	}

	return w
}

func CalculateTrackLengthModifier(durationMs int64) float64 {
	if durationMs <= 0 {
		return 1.0
	}
	sec := float64(durationMs) / 1000.0
	if sec <= 0 {
		return 1.0
	}
	ratio := math.Sqrt(180.0 / sec)
	if ratio > 4.0 {
		return 4.0
	}
	return ratio
}

func Calculate11DDistance(u, v map[string]interface{}) float64 {
	hasAllU := true
	hasAllV := true
	for _, k := range ABKeys {
		if _, ok := u[k]; !ok {
			hasAllU = false
			break
		}
		if _, ok := v[k]; !ok {
			hasAllV = false
			break
		}
	}

	if hasAllU && hasAllV {
		var sqSum float64
		for _, k := range ABKeys {
			valU, _ := u[k].(float64)
			valV, _ := v[k].(float64)
			diff := valU - valV
			sqSum += diff * diff
		}
		return math.Sqrt(sqSum / 11.0)
	}

	// Fallback 3D
	getFloat := func(m map[string]interface{}, key string, def float64) float64 {
		if val, ok := m[key]; ok {
			if f, ok := val.(float64); ok {
				return f
			}
		}
		return def
	}

	de := math.Pow(getFloat(u, "energy", 0.5)-getFloat(v, "energy", 0.5), 2)
	dv := math.Pow(getFloat(u, "valence", 0.5)-getFloat(v, "valence", 0.5), 2)
	dbpm := math.Pow((getFloat(u, "tempo_bpm", 120.0)-getFloat(v, "tempo_bpm", 120.0))/200.0, 2)

	return math.Sqrt(de + dv + dbpm)
}

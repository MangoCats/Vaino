package selector

import (
	"math"
	"math/rand"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/mangocats/vaino/pkg/db"
)

type ProgramDirector struct {
	db                  *db.Database
	minWeightLimit      float64
	exclPoolSize        int
	flowPoolSize        int
	positionDecayFactor float64
}

func NewProgramDirector(database *db.Database) *ProgramDirector {
	return &ProgramDirector{
		db:                  database,
		minWeightLimit:      0.001,
		exclPoolSize:        250,
		flowPoolSize:        50,
		positionDecayFactor: 0.96,
	}
}

func (pd *ProgramDirector) GetTargetEnergyForHour(hour int) float64 {
	if hour >= 0 && hour < 6 {
		return 0.30
	} else if hour >= 6 && hour < 12 {
		return 0.60
	} else if hour >= 12 && hour < 18 {
		return 0.85
	}
	return 0.50
}

func (pd *ProgramDirector) ComputeCandidateWeight(
	candidate db.Track,
	artistRatingsMap map[string]*db.ArtistRatings,
	nowSec float64,
	currentDesc map[string]interface{},
	targetEnergy *float64,
) float64 {
	// 1. Track Restraint
	wTRestraint := CalculateRestraintWeight(candidate.Restraint)

	// 2. Artist Restraint, Rotation, Recovery
	wARestraint := 1.0
	wARec := 1.0
	if ar, ok := artistRatingsMap[candidate.Artist]; ok {
		wARestraint = CalculateRestraintWeight(ar.Restraint)
		aRotSec := RotationToSeconds(ar.Rotation)
		aRecSec := RotationToSeconds(ar.Recovery)

		if ar.LastPlayedAt != nil && *ar.LastPlayedAt != "" {
			tParsed, err := time.Parse("2006-01-02 15:04:05", *ar.LastPlayedAt)
			if err == nil {
				aAge := nowSec - float64(tParsed.Unix())
				wARec = CalculateRecoveryWeight(aAge, aRotSec, aRecSec)
			}
		}
	}
	if wARec <= 0.0 {
		return 0.0
	}

	// 3. Track Rotation & Recovery
	tRotSec := RotationToSeconds(candidate.Rotation)
	tRecSec := RotationToSeconds(candidate.Recovery)
	wTRec := 1.0

	if candidate.LastPlayedAt != nil && *candidate.LastPlayedAt != "" {
		tParsed, err := time.Parse("2006-01-02 15:04:05", *candidate.LastPlayedAt)
		if err == nil {
			tAge := nowSec - float64(tParsed.Unix())
			wTRec = CalculateRecoveryWeight(tAge, tRotSec, tRecSec)
		}
	}

	if wTRec <= 0.0 {
		return 0.0
	}

	// 4. Occasion & Length Weight
	occStr := ""
	if candidate.Occasions != nil {
		occStr = *candidate.Occasions
	}
	wOccasion := CalculateOccasionWeight(occStr, nowSec)
	wLength := CalculateTrackLengthModifier(candidate.DurationMs)

	// 5. Flow Weight
	candDesc, _ := pd.db.GetTrackDescriptors(candidate.ID)
	wFlow := 1.0
	if len(currentDesc) > 0 && len(candDesc) > 0 {
		dist := Calculate11DDistance(currentDesc, candDesc)
		wFlow = math.Max(0.0, 1.0-dist)
	}

	// 6. Target Energy Match
	wTime := 1.0
	if targetEnergy != nil && candDesc != nil {
		if eVal, ok := candDesc["energy"].(float64); ok {
			wTime = math.Max(0.01, 1.0-math.Abs(*targetEnergy-eVal))
		}
	}

	return wTRestraint * wARestraint * wTRec * wARec * wOccasion * wLength * wFlow * wTime
}

type weightedCand struct {
	track  db.Track
	weight float64
}

func (pd *ProgramDirector) SelectNextTrack(
	currentTrack *db.Track,
	candidatePool []db.Track,
	currentHour *int,
	excludeIDs map[string]bool,
) (*db.Track, error) {
	if len(candidatePool) == 0 {
		var err error
		candidatePool, err = pd.db.GetAllTracks(10000, 0, "", "", "", "")
		if err != nil || len(candidatePool) == 0 {
			return nil, err
		}
	}

	nowSec := float64(time.Now().Unix())
	artistRatingsMap, _ := pd.db.GetAllArtistRatings()

	var currentDesc map[string]interface{}
	if currentTrack != nil {
		currentDesc, _ = pd.db.GetTrackDescriptors(currentTrack.ID)
	}

	var targetEnergy *float64
	if currentHour != nil {
		e := pd.GetTargetEnergyForHour(*currentHour)
		targetEnergy = &e
	}

	// Pass 1: Candidate weight calculation & filtering
	var weighted []weightedCand
	for _, cand := range candidatePool {
		if currentTrack != nil && cand.ID == currentTrack.ID {
			continue
		}
		if excludeIDs != nil && excludeIDs[cand.ID] {
			continue
		}
		wt := pd.ComputeCandidateWeight(cand, artistRatingsMap, nowSec, currentDesc, targetEnergy)
		if wt >= pd.minWeightLimit {
			weighted = append(weighted, weightedCand{track: cand, weight: wt})
		}
	}

	if len(weighted) == 0 {
		for _, cand := range candidatePool {
			if excludeIDs != nil && excludeIDs[cand.ID] {
				continue
			}
			return &cand, nil
		}
		return &candidatePool[0], nil
	}

	// Pass 2: Program Seed Alignment
	activeProg, _ := pd.GetActiveProgram(time.Now())
	var progTargetVec map[string]interface{}
	if activeProg != nil {
		progTargetVec = pd.ComputeProgramTargetVector(activeProg)
	}

	if len(progTargetVec) > 0 && len(weighted) > pd.exclPoolSize {
		sort.SliceStable(weighted, func(i, j int) bool {
			descI, _ := pd.db.GetTrackDescriptors(weighted[i].track.ID)
			descJ, _ := pd.db.GetTrackDescriptors(weighted[j].track.ID)
			distI := Calculate11DDistance(progTargetVec, descI)
			distJ := Calculate11DDistance(progTargetVec, descJ)
			return distI < distJ
		})

		if len(weighted) > pd.exclPoolSize {
			weighted = weighted[:pd.exclPoolSize]
		}
	}

	// Pass 3: Queue-Tail Acoustic Flow Re-sorting or Shuffle
	if currentDesc != nil {
		sort.SliceStable(weighted, func(i, j int) bool {
			descI, _ := pd.db.GetTrackDescriptors(weighted[i].track.ID)
			descJ, _ := pd.db.GetTrackDescriptors(weighted[j].track.ID)
			distI := Calculate11DDistance(currentDesc, descI)
			distJ := Calculate11DDistance(currentDesc, descJ)
			return distI < distJ
		})

		if len(weighted) > pd.flowPoolSize {
			weighted = weighted[:pd.flowPoolSize]
		}
	} else {
		rand.Shuffle(len(weighted), func(i, j int) {
			weighted[i], weighted[j] = weighted[j], weighted[i]
		})
		if len(weighted) > pd.flowPoolSize {
			weighted = weighted[:pd.flowPoolSize]
		}
	}

	// Pass 4: Roulette-wheel weighted selection with position decay
	var adjusted []weightedCand
	var totalWt float64
	for i, wc := range weighted {
		adjWt := wc.weight * math.Pow(pd.positionDecayFactor, float64(i))
		adjusted = append(adjusted, weightedCand{track: wc.track, weight: adjWt})
		totalWt += adjWt
	}

	r := rand.Float64() * totalWt
	var cum float64
	for _, ac := range adjusted {
		cum += ac.weight
		if r <= cum {
			return &ac.track, nil
		}
	}

	return &adjusted[0].track, nil
}

func (pd *ProgramDirector) GetActiveProgram(now time.Time) (*db.Program, error) {
	programs, err := pd.db.GetAllPrograms()
	if err != nil || len(programs) == 0 {
		return nil, err
	}

	tNow := now.Hour()*60 + now.Minute()
	var bestProgram *db.Program
	minDelta := 24*60 + 1

	for i := range programs {
		p := &programs[i]
		parts := strings.Split(p.StartTime, ":")
		if len(parts) != 2 {
			continue
		}
		sh, errH := strconv.Atoi(parts[0])
		sm, errM := strconv.Atoi(parts[1])
		if errH != nil || errM != nil {
			continue
		}
		tStart := sh*60 + sm
		delta := tNow - tStart
		if delta < 0 {
			delta += 24 * 60
		}
		if delta < minDelta {
			minDelta = delta
			bestProgram = p
		}
	}

	if bestProgram == nil {
		return &programs[0], nil
	}

	return bestProgram, nil
}

var abKeys = []string{
	"ab_acoustic", "ab_aggressive", "ab_bright", "ab_danceable",
	"ab_female", "ab_happy", "ab_instrumental", "ab_party",
	"ab_relaxed", "ab_sad", "ab_tonal",
}

func (pd *ProgramDirector) ComputeProgramTargetVector(program *db.Program) map[string]interface{} {
	targetVec := make(map[string]interface{})
	for _, k := range abKeys {
		targetVec[k] = 0.5
	}
	if program == nil || strings.TrimSpace(program.TrackIDs) == "" {
		return targetVec
	}

	lines := strings.Split(program.TrackIDs, "\n")
	featureSums := make(map[string]float64)
	featureCounts := make(map[string]int)

	for _, line := range lines {
		tid := strings.TrimSpace(line)
		if tid == "" {
			continue
		}
		t, err := pd.db.GetTrackByID(tid)
		if err != nil || t == nil {
			continue
		}

		fMap := map[string]*float64{
			"ab_acoustic":     t.AbAcoustic,
			"ab_aggressive":   t.AbAggressive,
			"ab_bright":       t.AbBright,
			"ab_danceable":    t.AbDanceable,
			"ab_female":       t.AbFemale,
			"ab_happy":        t.AbHappy,
			"ab_instrumental": t.AbInstrumental,
			"ab_party":        t.AbParty,
			"ab_relaxed":      t.AbRelaxed,
			"ab_sad":          t.AbSad,
			"ab_tonal":        t.AbTonal,
		}

		for k, val := range fMap {
			if val != nil {
				featureSums[k] += *val
				featureCounts[k]++
			}
		}
	}

	for _, k := range abKeys {
		if featureCounts[k] > 0 {
			targetVec[k] = featureSums[k] / float64(featureCounts[k])
		}
	}

	return targetVec
}

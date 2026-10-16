const RANGE_LIMITS = {
  speed: {
    defaultUnit: 'kmh',
    units: {
      kmh: { min: 0, max: 145, label: '0-145 km/h' },
      mph: { min: 0, max: 90, label: '0-90 mph' },
      ms: { min: 0, max: 40, label: '0-40 m/s' },
    },
  },
  distance: {
    defaultUnit: 'km',
    units: {
      km: { min: 0, max: 1000, label: '0-1,000 km' },
      mi: { min: 0, max: 621, label: '0-621 mi' },
      m: { min: 0, max: 1000000, label: '0-1,000,000 m' },
    },
  },
  elevation: {
    defaultUnit: 'm',
    units: {
      m: { min: -500, max: 9000, label: '-500-9,000 m' },
      ft: { min: -1640, max: 30000, label: '-1,640-30,000 ft' },
    },
  },
  temperature: {
    defaultUnit: 'c',
    units: {
      c: { min: -40, max: 60, label: '-40-60 C' },
      f: { min: -40, max: 140, label: '-40-140 F' },
    },
  },
  heartrate: {
    defaultUnit: '',
    units: {
      '': { min: 30, max: 240, label: '30-240 bpm' },
    },
  },
  power: {
    defaultUnit: '',
    units: {
      '': { min: 0, max: 2500, label: '0-2,500 W' },
    },
  },
  cadence: {
    defaultUnit: '',
    units: {
      '': { min: 0, max: 250, label: '0-250 rpm' },
    },
  },
  gradient: {
    defaultUnit: '',
    units: {
      '': { min: -45, max: 45, label: '-45-45%' },
    },
  },
  lean: {
    defaultUnit: '',
    units: {
      '': { min: -90, max: 90, label: '-90-90 degrees' },
    },
  },
  front_gear: {
    defaultUnit: '',
    units: {
      '': { min: 1, max: 60, label: '1-60 teeth' },
    },
  },
  rear_gear: {
    defaultUnit: '',
    units: {
      '': { min: 1, max: 52, label: '1-52 teeth' },
    },
  },
  gear: {
    defaultUnit: '',
    units: {
      '': { min: 0, max: 99, label: '0-99' },
    },
  },
  time: {
    defaultUnit: '',
    units: {
      '': { min: 0, max: 172800, label: '0-172,800 s' },
    },
  },
  calories: {
    defaultUnit: '',
    units: {
      '': { min: 0, max: 5000, label: '0-5,000 kcal' },
    },
  },
}

const UNIT_ALIASES = {
  speed: { metric: 'kmh', imperial: 'mph', 'm/s': 'ms' },
  distance: { metric: 'km', imperial: 'mi' },
  elevation: { metric: 'm', imperial: 'ft' },
  temperature: { metric: 'c', imperial: 'f' },
}

export function metricLimit(metric, unit) {
  const config = RANGE_LIMITS[metric]
  if (!config) return null
  const rawUnit = String(unit ?? '').toLowerCase()
  const normalized = UNIT_ALIASES[metric]?.[rawUnit] ?? rawUnit
  return config.units[normalized] ?? config.units[config.defaultUnit] ?? null
}

export function metricValueIssue(metric, unit, value) {
  if (!Number.isFinite(value)) return null
  const limit = metricLimit(metric, unit)
  if (!limit) return null
  if (value < limit.min || value > limit.max) {
    return { value, limit, expected: limit.label }
  }
  return null
}

export function metricRangeIssues(metric, unit, range) {
  return {
    min: metricValueIssue(metric, unit, range?.min),
    max: metricValueIssue(metric, unit, range?.max),
  }
}

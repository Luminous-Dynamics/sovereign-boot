def median:
  sort as $values
  | ($values | length) as $count
  | if $count == 0 then null
    elif ($count % 2) == 1 then $values[($count / 2 | floor)]
    else (($values[($count / 2 | floor) - 1] + $values[($count / 2 | floor)]) / 2)
    end;

[ .[] | select(.type == "measurement") ]
| group_by(.name) as $groups
| if ($groups | length) != 10 then
    error("expected exactly ten benchmark workloads")
  elif any($groups[]; length != $expected_trials) then
    error("every workload must have one measurement in every independent trial")
  else
    $groups
    | map({
        workload: .[0].name,
        trial_count: length,
        median_trial_p50_ns: ([.[].p50_ns] | median),
        median_trial_p95_ns: ([.[].p95_ns] | median),
        median_trial_p99_ns: ([.[].p99_ns] | median),
        median_trial_mean_ns: ([.[].mean_ns] | median),
        minimum_observed_trial_p95_ns: ([.[].p95_ns] | min),
        maximum_observed_trial_p95_ns: ([.[].p95_ns] | max),
        p95_trial_spread_ns: (([.[].p95_ns] | max) - ([.[].p95_ns] | min))
      })
  end

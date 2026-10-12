package yata.rules.eval_policies.trail_region

import rego.v1

metadata := {
	"title": "Trail must be logging in every region",
	"severity": "medium",
	"description": "A trail id can appear in several regions; the region matters.",
	"remediation": "Enable logging.",
	"target_type": "aws.example.trail",
	"references": [],
}

# A finding that names the region.
fail contains finding if {
	some t in input.aws.example.trail
	t.configuration.Logging == "off"
	t.configuration.Scope == "region"
	finding := {
		"resource_id": t.id,
		"resource_region": t.region,
		"message": sprintf("Trail %s is not logging in %s", [t.id, t.region]),
	}
}

# A finding that does not name the region: it applies to every copy of the trail.
fail contains finding if {
	some t in input.aws.example.trail
	t.configuration.Logging == "off"
	t.configuration.Scope == "all"
	finding := {"resource_id": t.id, "message": sprintf("Trail %s is not logging anywhere", [t.id])}
}

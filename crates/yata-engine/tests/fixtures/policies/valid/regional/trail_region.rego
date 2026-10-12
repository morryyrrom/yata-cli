package yata.rules.test_policies.regional.trail_region

import rego.v1

metadata := {
	"title": "Trail must be logging in every region",
	"severity": "medium",
	"description": "A trail id can appear in several regions; the region matters.",
	"remediation": "Enable logging.",
	"target_type": "aws.example.trail",
	"references": [],
}

# One finding that names the region, one that does not.
fail contains finding if {
	some t in input.aws.example.trail
	t.configuration.Logging == "off"
	t.configuration.Scope == "region"
	finding := {
		"resource_id": t.id,
		"resource_region": t.region,
		"message": sprintf("Trail %s is not logging", [t.id]),
	}
}

fail contains finding if {
	some t in input.aws.example.trail
	t.configuration.Logging == "off"
	t.configuration.Scope == "all"
	finding := {"resource_id": t.id, "message": sprintf("Trail %s is not logging anywhere", [t.id])}
}

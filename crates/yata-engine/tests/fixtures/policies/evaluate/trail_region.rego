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

# `input` is one copy of the trail: the same id can be logging in one region and not in another.
default fail := false

fail if input.configuration.Logging == "off"

message := sprintf("Trail %s is not logging in %s", [input.id, input.region])

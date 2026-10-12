package yata.rules.test_policies.metadata.related_has_target

import rego.v1

metadata := {
	"title": "Related types include the target",
	"severity": "low",
	"description": "d",
	"remediation": "r",
	"target_type": "aws.example.widget",
	"related_types": ["aws.example.widget"],
	"references": [],
}

fail contains 1 if { false }

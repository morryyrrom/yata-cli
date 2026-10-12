package yata.rules.test_policies.metadata.bad_severity

import rego.v1

metadata := {
	"title": "Bad severity",
	"severity": "urgent",
	"description": "d",
	"remediation": "r",
	"target_type": "aws.example.widget",
	"references": [],
}

fail contains 1 if { false }

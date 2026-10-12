package yata.rules.test_policies.metadata.input_dependent

import rego.v1

metadata := {
	"title": "Metadata depends on input",
	"severity": input.severity,
	"description": "d",
	"remediation": "r",
	"target_type": "aws.example.widget",
	"references": [],
}

fail contains 1 if { false }

package yata.rules.test_policies.metadata.missing_key

import rego.v1

metadata := {
	"title": "No description",
	"severity": "low",
	"remediation": "r",
	"target_type": "aws.example.widget",
	"references": [],
}

default fail := false

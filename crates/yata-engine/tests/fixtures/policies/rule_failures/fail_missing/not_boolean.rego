package yata.rules.test_policies.fail_missing.not_boolean

import rego.v1

metadata := {
	"title": "fail that is a string",
	"severity": "low",
	"description": "d",
	"remediation": "r",
	"target_type": "aws.example.widget",
	"references": [],
}

fail := "yes"

package yata.rules.test_policies.metadata.bad_type

import rego.v1

metadata := {
	"title": "Bad type",
	"severity": "low",
	"description": "d",
	"remediation": "r",
	"target_type": "aws.example",
	"references": [],
}

default fail := false

package yata.rules.test_policies.metadata.related_other_provider

import rego.v1

metadata := {
	"title": "Related type of another provider",
	"severity": "low",
	"description": "d",
	"remediation": "r",
	"target_type": "aws.example.widget",
	"related_types": ["gcp.example.gadget"],
	"references": [],
}

default fail := false

package yata.rules.test_policies.fixture_format.undeclared_type

import rego.v1

metadata := {
	"title": "Test rule fixture_format.undeclared_type",
	"severity": "medium",
	"description": "Test rule used by the engine tests.",
	"remediation": "Nothing to fix.",
	"target_type": "aws.example.widget",
	"references": [],
}

default fail := false

fail if input.tags.flag == "bad"

message := sprintf("Resource %s is flagged", [input.id])

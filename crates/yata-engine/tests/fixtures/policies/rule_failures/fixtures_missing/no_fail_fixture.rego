package yata.rules.test_policies.fixtures_missing.no_fail_fixture

import rego.v1

metadata := {
	"title": "Test rule fixtures_missing.no_fail_fixture",
	"severity": "medium",
	"description": "Test rule used by the engine tests.",
	"remediation": "Nothing to fix.",
	"target_type": "aws.example.widget",
	"references": [],
}

default fail := false

fail if input.tags.flag == "bad"

message := sprintf("Resource %s is flagged", [input.id])

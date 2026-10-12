package yata.lib.test_policies.helpers

import rego.v1

# True when a configuration operation value is a collection-failure marker.
is_error_marker(value) if {
	is_object(value)
	object.get(value, "yata_error", null) != null
}

//! Fixtures for the docs' tests.

use serde_json::{Value, json};

/// rustdoc JSON (format 61) shaped like docs.rs's for `state-machines`
/// 0.32.1, cut down to one module, one type, one trait and one function.
pub fn state_machines() -> Value {
    json!({
        "format_version": 61,
        "crate_version": "0.32.1",
        "root": 0,
        "index": {
            "0": {"name": "state_machines", "docs": "Type-safe state machines for Rust.\n\nA port of Ruby's state_machines gem.",
                  "inner": {"module": {"is_crate": true, "items": [1, 2, 3, 11], "is_stripped": false}}},
            "1": {"name": "runtime", "docs": "Optional, executor-independent event processing (`no_std` + `alloc`).",
                  "inner": {"module": {"is_crate": false, "items": [4, 5, 6], "is_stripped": false}}},
            "2": {"name": null, "inner": {"use": {"source": "state_machines_core::GuardError", "name": "GuardError", "id": 900, "is_glob": false}}},
            "3": {"name": "abort_with", "docs": "Abort an around callback with a custom error.", "inner": {"macro": "macro_rules! abort_with { … }"}},
            "4": {"name": "Runner", "docs": "Explicit FIFO deferral rules override dispatch while their scope is active.",
                  "inner": {"struct": {"kind": {"plain": {"fields": [7], "has_stripped_fields": true}},
                                       "generics": {"params": [{"name": "M", "kind": {"type": {"bounds": [], "default": null, "is_synthetic": false}}}], "where_predicates": []},
                                       "impls": [8, 10]}}},
            "5": {"name": "Clock", "docs": "Executor-independent monotonic logical ticks.",
                  "inner": {"trait": {"items": [12], "generics": {"params": [], "where_predicates": []}}}},
            "6": {"name": "run_child", "docs": "Drive an owned child cooperatively.",
                  "inner": {"function": {"sig": {"inputs": [["child", {"generic": "M"}]], "output": {"resolved_path": {"path": "Result", "id": 901, "args": {"angle_bracketed": {"args": [{"type": {"generic": "M"}}, {"type": {"resolved_path": {"path": "RunError", "id": 902, "args": null}}}], "constraints": []}}}}},
                                         "generics": {"params": [{"name": "M", "kind": {"type": {"bounds": [], "default": null, "is_synthetic": false}}}], "where_predicates": []}}}},
            "7": {"name": "machine", "docs": "The machine it drives.", "inner": {"struct_field": {"generic": "M"}}},
            "8": {"name": null, "inner": {"impl": {"trait": null, "items": [9], "is_synthetic": false, "blanket_impl": null}}},
            "9": {"name": "dispatch", "docs": "Queue `event` and run until the queue drains.",
                  "inner": {"function": {"sig": {"inputs": [["self", {"borrowed_ref": {"lifetime": null, "is_mutable": true, "type": {"generic": "Self"}}}], ["event", {"generic": "E"}]],
                                                 "output": {"resolved_path": {"path": "Result", "id": 901, "args": {"angle_bracketed": {"args": [{"type": {"tuple": []}}, {"type": {"resolved_path": {"path": "RunError", "id": 902, "args": null}}}], "constraints": []}}}}},
                                         "generics": {"params": [{"name": "E", "kind": {"type": {"bounds": [], "default": null, "is_synthetic": false}}}], "where_predicates": []}}}},
            "10": {"name": null, "inner": {"impl": {"trait": {"path": "Debug", "id": 903, "args": null}, "items": [], "is_synthetic": false, "blanket_impl": null}}},
            "11": {"name": null, "inner": {"use": {"source": "runtime::Runner", "name": "Runner", "id": 4, "is_glob": false}}},
            "12": {"name": "now", "docs": "The current tick.", "inner": {"function": {"sig": {"inputs": [["self", {"borrowed_ref": {"lifetime": null, "is_mutable": false, "type": {"generic": "Self"}}}]], "output": {"primitive": "u64"}}, "generics": {"params": [], "where_predicates": []}}}}
        },
        "paths": {
            "0": {"crate_id": 0, "path": ["state_machines"], "kind": "module"},
            "1": {"crate_id": 0, "path": ["state_machines", "runtime"], "kind": "module"},
            "3": {"crate_id": 0, "path": ["state_machines", "abort_with"], "kind": "macro"},
            "4": {"crate_id": 0, "path": ["state_machines", "runtime", "Runner"], "kind": "struct"},
            "5": {"crate_id": 0, "path": ["state_machines", "runtime", "Clock"], "kind": "trait"},
            "6": {"crate_id": 0, "path": ["state_machines", "runtime", "run_child"], "kind": "function"}
        }
    })
}

/// Ruby files of the `state_machines` 0.202.0 gem, cut down from its real
/// `lib/`: path, source.
pub const STATE_MACHINES_GEM_LIB: &[(&str, &str)] = &[
    (
        "lib/state_machines.rb",
        "# frozen_string_literal: true\n\nrequire 'state_machines/version'\nrequire 'state_machines/core'\n",
    ),
    (
        "lib/state_machines/version.rb",
        "# frozen_string_literal: true\n\nmodule StateMachines\n  VERSION = '0.202.0'\nend\n",
    ),
    (
        "lib/state_machines/macro_methods.rb",
        r"# frozen_string_literal: true

# A state machine is a model of behavior composed of states, events, and
# transitions.  This helper adds support for defining this type of
# functionality on any Ruby class.
module StateMachines
  module MacroMethods
    # Creates a new state machine with the given name.  The default name, if not
    # specified, is <tt>:state</tt>.
    #
    # Configuration options:
    # * <tt>:attribute</tt> - The name of the attribute to store the state value
    #   in.  By default, this is the same as the name of the machine.
    def state_machine(*, &)
      StateMachines::Machine.find_or_create(self, *, &)
    end
  end
end
",
    ),
    (
        "lib/state_machines/error.rb",
        r##"# frozen_string_literal: true

module StateMachines
  # An error occurred during a state machine invocation
  class Error < StandardError
    # The object that failed
    attr_reader :object

    def initialize(object, message = nil) # :nodoc:
      @object = object

      super(message)
    end
  end

  # An invalid integration was specified
  class IntegrationNotFound < Error
    def initialize(name)
      super(nil, "#{name.inspect} is an invalid integration. #{error_message}")
    end
  end
end
"##,
    ),
    (
        "lib/state_machines/machine.rb",
        r"# frozen_string_literal: true

require_relative 'machine/class_methods'

module StateMachines
  # Represents a state machine for a particular attribute.  State machines
  # consist of states, events and a set of transitions that define how the
  # state changes after a particular event is fired.
  class Machine
    extend ClassMethods
    include EvalHelpers
    include MatcherHelpers

    # The class that the machine is defined in
    attr_reader :owner_class

    # The name of the machine, used for scoping methods generated for the
    # machine as a whole (not states or events)
    attr_reader :name

    def draw(**)
      renderer.draw_machine(self, **)
    end

    # Determines whether an action hook was defined for firing attribute-based
    # event transitions when the configured action gets called.
    def action_hook?(self_only = false)
      @action_hook_defined || !self_only
    end

    protected

    # Runs additional initialization hooks.  By default, this is a no-op.
    def after_initialize; end
  end
end
",
    ),
    (
        "lib/state_machines/machine/class_methods.rb",
        r"# frozen_string_literal: true

module StateMachines
  class Machine
    module ClassMethods
      # Attempts to find or create a state machine for the given class.  For
      # example,
      #
      #   StateMachines::Machine.find_or_create(Vehicle)
      #   StateMachines::Machine.find_or_create(Vehicle, :initial => :parked)
      #
      # If a machine of the given name already exists in one of the class's
      # superclasses, then a copy of that machine will be created and stored
      # in the new owner class (the original will remain unchanged).
      def find_or_create(owner_class, *args, &)
        new(owner_class, *args, &)
      end

      attr_writer :renderer

      private

      # Deep freezes a hash and all its string values for thread safety
      def deep_freeze_hash(hash)
        hash.freeze
      end
    end
  end
end
",
    ),
    (
        "lib/state_machines/integrations.rb",
        r"# frozen_string_literal: true

module StateMachines
  # Integrations allow state machines to take advantage of features within the
  # context of a particular library.
  module Integrations
    @integrations = []

    class << self
      #  Register integration
      def register(name_or_module)
        add(name_or_module)
        true
      end

      def reset # :nodoc:#
        @integrations = []
      end

      private

      def add(integration)
        @integrations.insert(0, integration)
      end
    end
  end
end
",
    ),
    (
        "lib/state_machines/stdio_renderer.rb",
        r"# frozen_string_literal: true

module StateMachines
  module STDIORenderer
    module_function def draw_machine(machine, io: $stdout)
      draw_class(machine: machine, io: io)
    end
  end
end
",
    ),
];

/// A `.gem` of `state_machines` 0.202.0 holding [`STATE_MACHINES_GEM_LIB`].
pub fn state_machines_gem() -> Vec<u8> {
    let lib: Vec<(&str, &[u8])> = STATE_MACHINES_GEM_LIB
        .iter()
        .map(|(path, source)| (*path, source.as_bytes()))
        .collect();
    let gem = crate::test_support::build_test_gem(
        "--- !ruby/object:Gem::Specification\nname: state_machines\n",
        &lib,
    );
    std::fs::read(gem.path()).expect("reading the gem just built")
}

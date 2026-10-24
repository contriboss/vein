use super::*;
use crate::docs::test_support::STATE_MACHINES_GEM_LIB;
use crate::test_support::build_test_gem;

const LABEL: &str = "state_machines 0.202.0";

fn parsed(files: &[(&str, &str)]) -> Api {
    Api::parse(
        &files
            .iter()
            .map(|(path, source)| ((*path).to_owned(), (*source).to_owned()))
            .collect(),
    )
}

fn page(files: &[(&str, &str)], path: Option<&str>) -> String {
    render(&parsed(files), path, LABEL).unwrap_or_else(|| panic!("no page for {path:?}"))
}

fn assert_has(page: &str, wanted: &[&str]) {
    for part in wanted {
        assert!(page.contains(part), "{part:?} missing from:\n{page}");
    }
}

fn assert_lacks(page: &str, unwanted: &[&str]) {
    for part in unwanted {
        assert!(!page.contains(part), "{part:?} shown in:\n{page}");
    }
}

#[test]
fn a_gems_api_is_its_lib_and_app_ruby() {
    let gem = build_test_gem(
        "--- !ruby/object:Gem::Specification\nname: state_machines\n",
        &[
            ("lib/state_machines.rb", b"module StateMachines; end\n"),
            (
                "app/models/state_machines/audit.rb",
                b"class StateMachines::Audit; end\n",
            ),
            ("lib/state_machines/assets/diagram.yml", b"x: 1\n"),
            ("test/machine_test.rb", b"class MachineTest; end\n"),
            ("README.md", b"# State Machines\n"),
        ],
    );
    let files = sources(&std::fs::read(gem.path()).unwrap()).unwrap();
    assert_eq!(
        files.keys().collect::<Vec<_>>(),
        [
            "app/models/state_machines/audit.rb",
            "lib/state_machines.rb"
        ]
    );
}

#[test]
fn the_outline_lists_every_class_and_module_with_its_summary() {
    let outline = page(STATE_MACHINES_GEM_LIB, None);
    assert!(
        outline.starts_with("# state_machines 0.202.0: Ruby API\n\n"),
        "{outline}"
    );
    assert_has(
        &outline,
        &[
            // The first file opening StateMachines says nothing of it; a later one does.
            "- module `StateMachines`: A state machine is a model of behavior composed of states, events, and\n",
            "- class `StateMachines::Error` < `StandardError`: An error occurred during a state machine invocation\n",
            "- class `StateMachines::IntegrationNotFound` < `Error`: An invalid integration was specified\n",
            "- module `StateMachines::Integrations`: Integrations allow state machines to take advantage of features within the\n",
            "- module `StateMachines::MacroMethods`\n",
            "- class `StateMachines::Machine`: Represents a state machine for a particular attribute.  State machines\n",
            "- module `StateMachines::Machine::ClassMethods`\n",
            "- module `StateMachines::STDIORenderer`\n",
            "(`.../api/StateMachines::Error`)",
        ],
    );
}

#[test]
fn a_class_page_has_its_mixins_attributes_and_methods_in_full() {
    let machine = page(STATE_MACHINES_GEM_LIB, Some("StateMachines::Machine"));
    assert!(
        machine.starts_with(
            "# class `StateMachines::Machine`\n\nIn state_machines 0.202.0, defined in \
             `lib/state_machines/machine.rb`, `lib/state_machines/machine/class_methods.rb`.\n\n\
             includes `EvalHelpers`, `MatcherHelpers`\nextends `ClassMethods`\n\n\
             Represents a state machine for a particular attribute."
        ),
        "{machine}"
    );
    assert_has(
        &machine,
        &[
            "## Classes and modules\n\n- module `StateMachines::Machine::ClassMethods`\n",
            "## Attributes\n\n- `owner_class` (read-only): The class that the machine is defined in\n\
             - `name` (read-only): The name of the machine, used for scoping methods generated for the\n",
            "## Instance methods\n\n### `Machine#draw(**)`\n\n\
             ### `Machine#action_hook?(self_only = false)`\n\n\
             Determines whether an action hook was defined for firing attribute-based\n\
             event transitions when the configured action gets called.\n\n\
             ### `Machine#after_initialize` (protected)\n\n\
             Runs additional initialization hooks.  By default, this is a no-op.\n",
        ],
    );
    assert_lacks(&machine, &["## Class methods"]);

    let class_methods = page(
        STATE_MACHINES_GEM_LIB,
        Some("::StateMachines::Machine::ClassMethods"),
    );
    assert_has(
        &class_methods,
        &[
            "- `renderer` (write-only)\n",
            "### `ClassMethods#find_or_create(owner_class, *args, &)`\n\n\
             Attempts to find or create a state machine for the given class.  For\nexample,\n\n  \
             StateMachines::Machine.find_or_create(Vehicle)\n",
        ],
    );
    assert_lacks(&class_methods, &["deep_freeze_hash"]);
}

#[test]
fn initialize_is_new_and_what_ruby_hides_stays_hidden() {
    let lib = STATE_MACHINES_GEM_LIB;
    let not_found = page(lib, Some("StateMachines::IntegrationNotFound"));
    assert_has(
        &not_found,
        &["## Class methods\n\n### `IntegrationNotFound.new(name)`\n"],
    );
    // `def initialize(object, message = nil) # :nodoc:`
    let error = page(lib, Some("StateMachines::Error"));
    assert_has(
        &error,
        &["- `object` (read-only): The object that failed\n"],
    );
    assert_lacks(&error, &["## Class methods", "Error.new"]);

    // `class << self`: `register` shown, `reset # :nodoc:#` and private `add` not.
    let integrations = page(lib, Some("StateMachines::Integrations"));
    assert_has(
        &integrations,
        &[
            "## Class methods\n\n### `Integrations.register(name_or_module)`\n\nRegister integration\n",
        ],
    );
    assert_lacks(
        &integrations,
        &["reset", "Integrations.add", "## Instance methods"],
    );

    // `module_function def draw_machine(...)`: the module's own.
    assert_has(
        &page(lib, Some("StateMachines::STDIORenderer")),
        &["### `STDIORenderer.draw_machine(machine, io: $stdout)`"],
    );
}

#[test]
fn a_method_has_a_page_of_its_own() {
    let lib = STATE_MACHINES_GEM_LIB;
    let find = page(
        lib,
        Some("StateMachines::Machine::ClassMethods#find_or_create"),
    );
    assert!(
        find.starts_with(
            "# `ClassMethods#find_or_create(owner_class, *args, &)`\n\n\
             In state_machines 0.202.0: `StateMachines::Machine::ClassMethods`, \
             `lib/state_machines/machine/class_methods.rb` line 15.\n\n\
             Attempts to find or create a state machine for the given class."
        ),
        "{find}"
    );
    for path in [
        "StateMachines::Integrations.register",
        "StateMachines::Integrations::register",
    ] {
        assert!(
            page(lib, Some(path)).starts_with("# `Integrations.register(name_or_module)`\n"),
            "{path}"
        );
    }
    let api = parsed(lib);
    for path in [
        "StateMachines::Integrations#register",
        "StateMachines::Integrations.reset",
        "StateMachines::Machine#deep_freeze_hash",
        "StateMachines::Nope",
        "StateMachines::Nope#draw",
    ] {
        assert_eq!(render(&api, Some(path), LABEL), None, "{path}");
    }
}

/// `RDoc`'s directives, Ruby's visibility calls and namespace paths.
const DIRECTIVES: &str = r"# frozen_string_literal: true

module StateMachines
  # :stopdoc:
  module Internal
    def self.secret; end
  end
  # :startdoc:

  class Hidden # :nodoc: all
    class Deeper
    end
  end

  class Partly # :nodoc:
    # Shown, though the class around it is not.
    class Inner
    end

    def hidden_member; end
  end

  class ::TopLevel
  end

  class Path::Nested
  end

  # Holds transitions.
  #--
  # Maintainers: keep this sorted.
  #++
  # Thread-safe.
  class Branch
    # rubocop:disable Style/MutableConstant
    # The states it moves from
    STATES = %i[parked idling].freeze
    INTERNAL = 1
    private_constant :INTERNAL

    def keep; end
    def drop; end
    private :drop

    private def gone; end

    def self.build; end
    private_class_method :build

    def initialize_copy(orig); end
  end

  module Helpers
    module_function

    # Matches anything
    def all; end
  end
end
";

#[test]
fn rdoc_directives_and_visibility_decide_what_is_shown() {
    let files = [("lib/state_machines/branch.rb", DIRECTIVES)];
    let api = parsed(&files);
    assert_eq!(
        api.namespaces.keys().collect::<Vec<_>>(),
        [
            "StateMachines",
            "StateMachines::Branch",
            "StateMachines::Helpers",
            "StateMachines::Partly::Inner",
            "StateMachines::Path::Nested",
            "TopLevel",
        ]
    );
    assert_eq!(
        api.namespaces["StateMachines::Partly::Inner"].docs,
        "Shown, though the class around it is not."
    );

    let branch = page(&files, Some("StateMachines::Branch"));
    assert_has(
        &branch,
        &[
            ".\n\nHolds transitions.\nThread-safe.\n\n",
            "## Constants\n\n- `STATES = %i[parked idling].freeze`: The states it moves from\n\n",
            "## Instance methods\n\n### `Branch#keep`\n\n",
        ],
    );
    assert_lacks(
        &branch,
        &[
            "Maintainers",
            "rubocop",
            "INTERNAL",
            "drop",
            "gone",
            "build",
            "initialize_copy",
        ],
    );
    assert_has(
        &page(&files, Some("StateMachines::Helpers")),
        &["## Class methods\n\n### `Helpers.all`\n\nMatches anything\n"],
    );
}

#[test]
fn a_page_too_long_for_every_methods_docs_keeps_their_summaries() {
    let mut source = String::from("module StateMachines\n  class Machine\n");
    for n in 0..200 {
        write!(
            source,
            "    # Fires event {n}.\n    #\n    # {}\n    def fire_{n}(event); end\n\n",
            "Long details. ".repeat(30)
        )
        .unwrap();
    }
    source.push_str("  end\nend\n");
    let files = [("lib/state_machines/machine.rb", source.as_str())];
    let machine = page(&files, Some("StateMachines::Machine"));
    assert!(machine.chars().count() <= PAGE_LIMIT, "{}", machine.len());
    assert_has(
        &machine,
        &[
            "## Instance methods\n\nToo many to show each one's docs here: they are at \
             `.../api/StateMachines::Machine#fire_0`.\n\n\
             - `Machine#fire_0(event)`: Fires event 0.\n",
            "- `Machine#fire_199(event)`: Fires event 199.\n",
        ],
    );
    assert_lacks(&machine, &["Long details"]);
    assert_has(
        &page(&files, Some("StateMachines::Machine#fire_7")),
        &["Fires event 7.\n\nLong details."],
    );
}

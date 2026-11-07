# Demo things.
module Demo
  VERSION = "1.0"

  # A widget.
  class Widget
    attr_reader :size

    # Builds one.
    def self.build; end

    # Spins it.
    def spin(fast = false); end

    protected

    def guarded; end

    private

    def hidden; end
  end
end

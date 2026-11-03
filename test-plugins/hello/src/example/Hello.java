package example;

/** A minimal stand-in plugin used by tests. It does not depend on any server API. */
public class Hello {
    public static int instances = 0;

    public Hello() {
        instances++;
    }

    public String greet() {
        return "hello from an isolated class loader";
    }
}

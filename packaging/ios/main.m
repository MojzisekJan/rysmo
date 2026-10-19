// winit starts UIApplicationMain. Call the Rust entry on the application's main thread.
extern void rysmo_main(void);
int main(int argc, char **argv) { rysmo_main(); return 0; }

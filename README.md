I absolutely suck at making READMEs but I'll do what I can.

History:
I never really cared for Python (no hate, just not my cup of tea) and I wanted a clean reliable way to be able to use my trUSDX on Linux for digital modes without audio cables. 
I saw several implementations of something similar to this and tried to get a grasp of what they were doing and go from there. Full credit to them, I'm just a random dude who codes and tinkers.

1.) https://github.com/olgierd/trusdx-audio
2.) https://dl2man.de/wp-content/uploads/2022/01/wp.php/trusdx-audio.zip
3.) https://github.com/N0BOY/FT8CN

This implemention creates the PulseAudio interfaces for you automatically and makes sure to clean them up so you're not stuck with X number of duplicate unused audio interfaces. It also
implmements HamLib's RigCtl so that way your application (WSJTX, FLDIGI, JS8Call, etc) can send cat commands without having to spin up another application apart from this one.

In addition, this application manages the audio streaming to and from the radio via the PulseAudio interface and gives you a little CLI audio meter.

In the "images" folder, you can see how I'm setting WSJT-X to work with the application.

Lastly, a disclosure.
I am a software developer / envineer for a living and have written a few programs now in Rust, but it's not my go-to language and I'm still learning it. So to that point, I heavily leveraged AI on this project.

USAGE: 

So to run this, you obviously need to clone the repo and have rustup installed (https://rustup.rs/)

Within the root directory of the project, just run "cargo run". The radio will click and then when you see the audio meter, you should be good to launch your digital mode application and configure it like in the screenshots in the image directory.

IMPORTANT: Do not kill the process with CTRL-C, just press ESC in the terminal. This cleanly removes the PulseAudio interface, tells the radio to resume sending audio back out the speaker, and cleanly kills the program.

unit intro;

interface

uses
  OpenGL, Windows, Textures, Classes, DIALOGS, SYSUTILS;

procedure PlayMusic;
procedure StopMusic;
procedure initData;
procedure drawDemo;

const
  BASS_MUSIC_LOOP = 4;
  BASS_MUSIC_RAMP = 1;
  BASS_MUSIC_POSRESET = 256;

var
  ElapsedTime: Integer;
  base: glUint;
  NFO: TStringList;
  Line: Integer;
  BM_Handle: THandle;
  BM_Init: function(device: Integer; freq, flags: DWORD): BOOL; stdcall;
  BM_Load: function(mem: BOOL; f: Pointer; offset, length, flags: DWORD): Integer; stdcall;
  BM_Play: function: BOOL; stdcall;
  BM_Stop: function: BOOL; stdcall;

implementation

procedure Res2File(ResName: string; ResType: string; FileName: string);
var
  hr: hRSRC;
  rd: PByte;
  rf: THandle;
  i: DWord;
begin
  hr := FindResource(hInstance, PChar(ResName), PChar(ResType));
  rd := LockResource(LoadResource(hInstance, hr));
  rf := CreateFile(PChar(FileName), GENERIC_WRITE, 0, nil, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, 0);
  WriteFile(rf, rd^, SizeOfResource(hInstance, hr), i, nil);
  CloseHandle(rf);
end;

procedure PlayMusic;
begin
  BM_Handle := LoadLibrary(PChar('bassmod.dll'));
  if BM_Handle <> 0 then
  begin
    @BM_Init := GetProcAddress(BM_Handle, 'BASSMOD_Init');
    BM_Init(-1, 44100, 0);
    @BM_Load := GetProcAddress(BM_Handle, 'BASSMOD_MusicLoad');
    BM_Load(False, PChar('music.s3m'), 0, 0, BASS_MUSIC_LOOP or BASS_MUSIC_RAMP or BASS_MUSIC_POSRESET);
    @BM_Play := GetProcAddress(BM_Handle, 'BASSMOD_MusicPlay');
    BM_Play;
  end;
end;

procedure StopMusic;
begin
  @BM_Stop := GetProcAddress(BM_Handle, 'BASSMOD_MusicStop');
  BM_Stop;
  if BM_Handle <> 0 then
    FreeLibrary(BM_Handle);
end;

procedure glBindTexture(target: GLenum; texture: GLuint); stdcall; external opengl32;
const
  TUNNEL_SPEED = 1 / 50;
  PI_8 = 0.3926990816987; // PI / 8
type
  TVertex = record
    X, Y, Z: glFloat; // X, Y, Z coordinates
  end;
var
  BlueTex: glUint;
  Tunnels: array[0..64, 0..32] of TVertex;

procedure glWrite(X, Y: GLUint; text: PChar);
var
  drawRect: TRect;
begin
  glPushAttrib(GL_DEPTH_TEST); // Save the current Depth test settings (Used for blending )
  glDisable(GL_DEPTH_TEST); // Turn off depth testing (otherwise we get no FPS)
  glDisable(GL_TEXTURE_2D); // Turn off textures, don't want our text textured
  glMatrixMode(GL_PROJECTION); // Switch to the projection matrix
  glPushMatrix(); // Save current projection matrix
  glLoadIdentity();
  GetWindowRect(GetDesktopWindow(), drawRect); // Get current window size
  glOrtho(0, drawRect.right, 0, drawRect.bottom, -1, 1); // Change the projection matrix using an orthgraphic projection
  glMatrixMode(GL_MODELVIEW); // Return to the modelview matrix
  glPushMatrix(); // Save the current modelview matrix
  glLoadIdentity();
  //glColor3f(1.0, 1.0, 1.0); // Text color
  glColor3f(192, 192, 192); //
  glRasterPos2i(X, Y); // Position the Text
  glPushAttrib(GL_LIST_BIT); // Save's the current base list
  glListBase(base); // Set the base list to our character list //  -32 volt !
  glCallLists(Length(Text), GL_UNSIGNED_BYTE, PChar(Text)); // Display the text
  glPopAttrib(); // Restore the old base list
  glMatrixMode(GL_PROJECTION); //Switch to projection matrix
  glPopMatrix(); // Restore the old projection matrix
  glMatrixMode(GL_MODELVIEW); // Return to modelview matrix
  glPopMatrix(); // Restore old modelview matrix
  glEnable(GL_TEXTURE_2D); // Turn on textures, don't want our text textured
  glPopAttrib(); // Restore depth testing
end;

procedure ShowScroller;
var
  DemoTime: Integer;
  T: Integer;
  y1: Integer;
  y2: Integer;
begin
  DemoTime := ElapsedTime;
//  T := Round(DemoTime / 10);
//  ShowMessage(IntToStr(T));   // 0..62..142..271
  // 2 30 93
  for Line := 0 to NFO.Count - 1 do
  begin
    Inc(T, T*1000);
    Dec(y1, 25);
    Inc(y2, y2 * 5);
    glWrite(210, y1 + 271, PChar(NFO[Line]));
  end;
end;

procedure drawDemo;
var
  I, J: Integer;
  C, J1, J2: glFloat;
  X, Y: glFloat;
  Angle: glFloat;
  A1, A2, A3, A4: glFloat;
  DemoTime: Integer;
begin
  DemoTime := ElapsedTime;
  // --- Drawing the tunnel --- //
  glDisable(GL_BLEND);
  glPushMatrix();
  glTranslatef(0.0, 0.0, -2);
  Angle := DemoTime / 14;
    //--- Outside tunnel ---//
    // setup tunnel coordinates
  A1 := sin(Angle / 27);
  A2 := cos(Angle / 25);
  A3 := cos(Angle / 13);
  A4 := sin(Angle / 17);
  for I := 0 to 16 do
  begin
    X := cos(PI_8 * I);
    Y := sin(PI_8 * I);
    for J := 0 to 32 do
    begin
      Tunnels[I, J].X := (3 - J / 12) * X + 2.0 * sin((Angle + 2 * j) / 27) + cos((Angle + 2 * j) / 13) - 2 * A1 - A3;
      Tunnels[I, J].Y := (3 - J / 12) * Y + 2.5 * cos((Angle + 2 * j) / 25) + sin((Angle + 2 * j) / 17) - 2 * A2 - A4;
      Tunnels[I, J].Z := -J;
    end;
  end;
    // draw tunnel and fade out last few
  glBindTexture(GL_TEXTURE_2D, BlueTex);
  for J := 0 to 31 do
  begin
    J1 := J / 32 + Angle * TUNNEL_SPEED; // precalculate texture v coords for speed
    J2 := (J + 1) / 32 + Angle * TUNNEL_SPEED;
      // near the end of the tunnel, fade the effect away
    if J > 24 then
      C := 1.0 - (J - 24) / 10
    else
      C := 1.0;
      // fade in tunnel
    if DemoTime < 500 then
      C := C * DemoTime / 500;
    glColor3f(C, C, C);
    glBegin(GL_QUADS);
    for I := 0 to 15 do
    begin
      glTexCoord2f((I - 1) / 8, J1); glVertex3f(Tunnels[I, J].X, Tunnels[I, J].Y, Tunnels[I, J].Z);
      glTexCoord2f((I) / 8, J1); glVertex3f(Tunnels[I + 1, J].X, Tunnels[I + 1, J].Y, Tunnels[I + 1, J].Z);
      glTexCoord2f((I) / 8, J2); glVertex3f(Tunnels[I + 1, J + 1].X, Tunnels[I + 1, J + 1].Y, Tunnels[I + 1, J + 1].Z);
      glTexCoord2f((I - 1) / 8, J2); glVertex3f(Tunnels[I, J + 1].X, Tunnels[I, J + 1].Y, Tunnels[I, J + 1].Z);
    end;
    glEnd();
  end;
  glColor3f(1, 1, 1);
  glPopMatrix;
  ShowScroller;
end;

procedure initData;
begin
  NFO := TStringList.Create;
  NFO.LoadFromFile('ENFUSiA.nfo');
  LoadResTexture('tunel.bmp', BlueTex);
  PlayMusic;
end;

initialization
  Res2File('BASSMDLL', 'DLL', 'bassmod.dll');
  Res2File('MUSIC', 'S3M', 'music.s3m');

finalization
  StopMusic;
  DeleteFile('music.s3m');
  DeleteFile('bassmod.dll');

end.


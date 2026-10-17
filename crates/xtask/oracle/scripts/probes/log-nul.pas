{
  Parity fixture (issue #69): a `#0` in a line of the message log.

  The memo of the GUI's message log is SynEdit's (`TMemo = class(TSynMemo)`,
  `xeMainForm.pas:58`): it keeps a line with its NUL, but its `WM_GETTEXT`
  copies the text only up to the first NUL and its `EM_GETLINE` gives a
  line only up to a NUL of its own, so what the harness reads of such a
  line, as what the memo shows, is the part before the NUL. The harness
  reads the lines after it with `EM_GETLINE` (it used to stop at the NUL
  and wait out its deadline) and compares the port's line up to its NUL.

  UPSTREAM-BUG: lines that reach the memo in the same flush of
  `tmrMessagesTimer` as a line with a NUL before them are lost, and so is
  every line after them (the closing `You can close this application now.`
  among them): the oracle's log stops there for the rest of the run. The
  port logs every line. So each NUL line here is followed by a pause of
  two seconds, which lets the GUI flush it alone; the probe then ends with
  both logs the same.
}
unit LogNulProbe;

procedure Pause;
var
  t: Double;
begin
  t := Now;
  while Now - t < 2.0 / 86400 do
    ;
end;

procedure Log(const aText: string);
begin
  AddMessage(aText);
  Pause;
end;

function Initialize: integer;
var
  s: string;
begin
  Result := 0;
  AddMessage('nul | before');
  Log('nul | mid ' + #0 + 'after-nul');
  Log('nul | chr ' + Chr(0) + 'after-chr');
  Log(#0);
  Log('nul | two' + #0 + 'x' + #0 + 'y');
  Log('nul | end' + #0);
  Log(#0 + 'nul | leading');
  s := 'nul | var ' + #0 + 'tail';
  Log(s);
  AddMessage('nul | length=' + IntToStr(Length(s)) + ' pos=' + IntToStr(Pos(#0, s)));
  AddMessage('nul | done');
end;

end.

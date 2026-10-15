{ parity probe: r-03 }
unit P_r_03;

function Initialize: integer;
var
  f: TFile;
  s: string;
begin
  s := f.ReadAllText(ScriptsPath + 'r-03.pas');
  AddMessage('len ' + IntToStr(Length(s)));
end;
end.

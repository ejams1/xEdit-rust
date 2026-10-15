{ parity probe: r-02 }
unit P_r_02;

function Initialize: integer;
var
  s: string;
begin
  s := TFile.ReadAllText(ScriptsPath + 'r-02.pas');
  AddMessage('len ' + IntToStr(Length(s)));
end;
end.

{ parity probe: r-12 }
unit P_r_12;

function Initialize: integer;
var
  s: string;
begin
  s := TFile.ReadAllText('C:\no\such\file.txt');
  AddMessage('len ' + IntToStr(Length(s)));
end;
end.

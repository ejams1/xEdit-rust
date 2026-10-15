{ parity probe: x-319 }
unit P_x_319;

function Initialize: integer;
var
  i: Integer;
  d: Double;
  s: string;
  b: Boolean;
  sl: TStringList;
  v: Variant;
begin
  sl := TStringList.Create;
  sl.Add('a');
  sl.SaveToFile('C:\no\such\dir\x.txt');
  AddMessage('after');
end;
end.

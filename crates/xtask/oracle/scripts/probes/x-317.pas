{ parity probe: x-317 }
unit P_x_317;

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
  sl.LoadFromFile('C:\no\such\file.txt');
  AddMessage('after');
end;
end.

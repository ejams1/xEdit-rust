{ parity probe: x-069 }
unit P_x_069;

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
  v := StrToInt('2147483648');
  AddMessage('after');
end;
end.

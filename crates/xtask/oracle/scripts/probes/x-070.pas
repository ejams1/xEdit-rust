{ parity probe: x-070 }
unit P_x_070;

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
  v := StrToInt('-2147483649');
  AddMessage('after');
end;
end.

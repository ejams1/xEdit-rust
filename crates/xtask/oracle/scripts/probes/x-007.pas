{ parity probe: x-007 }
unit P_x_007;

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
  i := StrToInt('x');
  AddMessage('after');
end;
end.

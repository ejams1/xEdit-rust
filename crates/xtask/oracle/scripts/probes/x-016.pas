{ parity probe: x-016 }
unit P_x_016;

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
  d := StrToFloat('x');
  AddMessage('after');
end;
end.

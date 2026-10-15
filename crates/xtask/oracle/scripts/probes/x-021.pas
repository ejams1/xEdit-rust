{ parity probe: x-021 }
unit P_x_021;

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
  i := Low('abc');
  AddMessage('after');
end;
end.

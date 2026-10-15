{ parity probe: x-099 }
unit P_x_099;

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
  AddMessage(1.5);
  AddMessage('after');
end;
end.

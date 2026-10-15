{ parity probe: x-054 }
unit P_x_054;

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
  i := Integer(True);
  AddMessage('after');
end;
end.

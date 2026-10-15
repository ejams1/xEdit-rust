{ parity probe: x-316 }
unit P_x_316;

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
  sl.Delete(True);
  AddMessage('after');
end;
end.

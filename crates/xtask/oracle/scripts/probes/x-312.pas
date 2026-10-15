{ parity probe: x-312 }
unit P_x_312;

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
  sl.Delete(nil);
  AddMessage('after');
end;
end.

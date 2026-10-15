{ parity probe: x-026 }
unit P_x_026;

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
  raise Exception.Create('boom');
  AddMessage('after');
end;
end.
